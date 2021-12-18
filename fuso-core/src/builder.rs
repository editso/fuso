use std::{collections::HashMap, sync::Arc};

use fuso_api::{Advice, AsyncTcpSocketEx, Cipher, FusoAuth, FusoPacket, Spawn};
use fuso_socks::{AsyncUdpForward, PasswordAuth, Socks, Socks5Ex};
use futures::TryStreamExt;
use smol::{
    channel::unbounded,
    io::AsyncWriteExt,
    lock::{Mutex, RwLock},
    net::TcpStream,
};

use crate::{
    core::{Context, Fuso, FusoProxy, GlobalConfig, Session},
    dispatch::{Dispatch, DynHandler, SafeTcpStream, State},
    handler::ChainHandler,
    handsnake::{Handsnake, HandsnakeEx},
    packet::Action,
};

pub type DynCipher = dyn Cipher + Send + Sync + 'static;

pub struct FusoBuilder<C> {
    pub auth: Option<Arc<dyn FusoAuth<SafeTcpStream> + Send + Sync + 'static>>,
    pub ciphers: HashMap<
        String,
        Arc<
            Box<dyn Fn(Option<String>) -> fuso_api::Result<Box<DynCipher>> + Send + Sync + 'static>,
        >,
    >,
    pub config: Option<GlobalConfig>,
    pub handlers: Vec<Arc<Box<DynHandler<C, ()>>>>,
    pub strategys: Vec<Arc<Box<DynHandler<Arc<Session>, Action>>>>,
    pub handsnakes: Vec<Handsnake>,
    pub advices: Vec<Arc<dyn Advice<SafeTcpStream, Box<DynCipher>> + Send + Sync + 'static>>,
}

impl FusoBuilder<Arc<Context>> {
    #[inline]
    pub fn use_global_config(mut self, config: GlobalConfig) -> Self {
        self.config = Some(config);
        self
    }

    #[inline]
    pub fn chain_handler<F>(mut self, with_chain: F) -> Self
    where
        F: FnOnce(
            ChainHandler<SafeTcpStream, Arc<Context>, fuso_api::Result<State<()>>>,
        )
            -> ChainHandler<SafeTcpStream, Arc<Context>, fuso_api::Result<State<()>>>,
    {
        self.handlers
            .push(Arc::new(Box::new(with_chain(ChainHandler::new()))));
        self
    }

    #[inline]
    pub fn chain_strategy<F>(mut self, with_chain: F) -> Self
    where
        F: FnOnce(
            ChainHandler<SafeTcpStream, Arc<Session>, fuso_api::Result<State<Action>>>,
        )
            -> ChainHandler<SafeTcpStream, Arc<Session>, fuso_api::Result<State<Action>>>,
    {
        self.strategys
            .push(Arc::new(Box::new(with_chain(ChainHandler::new()))));

        self
    }

    pub fn use_auth<A>(mut self, auth: A) -> Self
    where
        A: FusoAuth<SafeTcpStream> + Send + Sync + 'static,
    {
        self.auth = Some(Arc::new(auth));

        self
    }

    pub fn add_handsnake(mut self, handsnake: Handsnake) -> Self {
        self.handsnakes.push(handsnake);
        self
    }

    pub fn add_cipher<N, F, C>(mut self, cipher_name: N, create_cipher: F) -> Self
    where
        N: Into<String>,
        F: Fn(Option<String>) -> fuso_api::Result<C> + Send + Sync + 'static,
        C: Cipher + Send + Sync + 'static,
    {
        self.ciphers.insert(
            cipher_name.into(),
            Arc::new(Box::new(move |secret| {
                let cipher = create_cipher(secret)?;
                Ok(Box::new(cipher))
            })),
        );

        self
    }

    pub fn add_advice<A>(mut self, advice: A) -> Self
    where
        A: Advice<SafeTcpStream, Box<DynCipher>> + Send + Sync + 'static,
    {
        self.advices.push(Arc::new(advice));
        self
    }

    pub fn use_default_handler(self) -> Self {
        self.chain_handler(|chain| {
            chain
                .next(|mut tcp, cx| async move {
                    for handsnake in cx.handsnakes.iter() {
                        let _ = tcp.begin().await;

                        if tcp.handsnake(handsnake).await? {
                            let _ = tcp.release().await;
                        } else {
                            let _ = tcp.back().await;
                        }
                    }

                    Ok(State::Next)
                })
                .next(|mut tcp, cx| async move {
                    for advice in cx.advices.iter() {
                        let _ = tcp.begin().await.unwrap();

                        let cipher = advice.advice(&mut tcp).await?;

                        if let Some(cipher) = cipher {
                            tcp.set_cipher(cipher).unwrap();
                            let _ = tcp.release().await;
                            break;
                        } else {
                            let _ = tcp.back().await;
                        }
                    }

                    Ok(State::Next)
                })
                .next(|mut tcp, cx| async move {
                    let _ = tcp.begin().await;

                    let action: Action = tcp.recv().await?.try_into()?;

                    match action {
                        Action::TcpBind(cfg) => {
                            let auth = cx.get_auth();

                            if let Some(auth) = auth.as_ref() {
                                let _ = auth.auth(&mut tcp).await?;
                            }

                            let state = cx.spawn(tcp.clone(), cfg).await;

                            if state.is_err() {
                                let err = state.unwrap_err().to_string();
                                log::warn!("[fuso] Failed to open the mapping {}", err);
                                let _ = tcp.send(Action::Err(err).into()).await?;
                            } else {
                                log::debug!(
                                    "[fuso] accept conv={}, addr={}",
                                    state.unwrap(),
                                    tcp.peer_addr().unwrap(),
                                );
                            }

                            Ok(State::Accept(()))
                        }
                        Action::UdpBind(conv) => {
                            cx.route(conv, action, tcp).await?;
                            Ok(State::Accept(()))
                        }
                        Action::Connect(conv, _) => {
                            cx.route(conv, action, tcp).await?;
                            Ok(State::Accept(()))
                        }
                        _ => {
                            let _ = tcp.back().await;
                            Ok(State::Next)
                        }
                    }
                })
        })
    }

    pub fn use_default_strategy(self) -> Self {
        self.chain_strategy(|chain| {
            chain
                .next(|mut tcp, core| async move {
                    if core.test("socks5") {
                        let _ = tcp.begin().await;
                        match tcp
                            .clone()
                            .authenticate({
                                match core.config.socks_passwd.as_ref() {
                                    None => None,
                                    Some(password) => {
                                        Some(Arc::new(PasswordAuth(password.clone())))
                                    }
                                }
                            })
                            .await
                        {
                            Ok(Socks::Udp(forward)) => {
                                log::info!("[udp_forward] {}", tcp.peer_addr().unwrap());

                                core.udp_forward(|listen, mut udp| {
                                    forward.resolve(listen.local_addr(), || async move {
                                        let mut stream = listen.accept().await?;
                                        let packet = stream.recv_udp().await?;

                                        let data =
                                            udp.call(packet.addr().into(), packet.data()).await?;

                                        stream.send_udp(([0, 0, 0, 0], 0).into(), &data).await?;

                                        Ok(())
                                    })
                                })
                                .await?;

                                Ok(State::Release)
                            }
                            Ok(Socks::Tcp(mut tcp, addr)) => {
                                let _ = tcp.release();
                                Ok(State::Accept(Action::Forward(0, addr.into())))
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                                let _ = tcp.back().await;
                                Ok(State::Next)
                            }
                            Err(e) => Err(e.into()),
                        }
                    } else {
                        Ok(State::Next)
                    }
                })
                .next(|_, _| async move {
                    Ok(State::Accept(Action::Forward(
                        0,
                        crate::packet::Addr::Socket(([0, 0, 0, 0], 0).into()),
                    )))
                })
        })
    }

    pub async fn build(self) -> fuso_api::Result<Fuso<FusoProxy<fuso_api::SafeStream<TcpStream>>>> {
        let config = Arc::new(self.config.unwrap());

        let (accept_ax, accept_tx) = unbounded();

        let bind_addr = config.bind_addr.clone();

        let listen = {
            let bind_addr = bind_addr.clone();
            bind_addr.tcp_listen().await?
        };

        let handlers = Arc::new(self.handlers);
        let strategys = Arc::new(self.strategys);
        let handsnakes = Arc::new(self.handsnakes);

        let cx = Arc::new(Context {
            config,
            accept_ax,
            handlers,
            strategys,
            handsnakes,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            alloc_conv: Arc::new(Mutex::new(0)),
            auth: self.auth,
            ciphers: Arc::new(self.ciphers),
            advices: self.advices,
        });

        async move {
            log::info!("Service started successfully");
            log::info!("Bind to {}", bind_addr);
            log::info!("Waiting to connect");

            let _ = listen
                .incoming()
                .try_fold(cx, |cx, mut tcp| async move {
                    log::debug!("[tcp] accept {}", tcp.peer_addr().unwrap());

                    {
                        let handlers = cx.handlers.clone();
                        let cx = cx.clone();
                        async move {
                            let process = tcp.clone().dispatch(handlers, cx).await;

                            if process.is_err() {
                                let _ = tcp.close().await;

                                log::warn!(
                                    "[tcp] An illegal connection {}",
                                    tcp.peer_addr()
                                        .map_or_else(|_| "Unknown".into(), |e| e.to_string())
                                );
                            } else {
                                log::debug!(
                                    "[tcp] Successfully processed {}",
                                    tcp.local_addr().unwrap()
                                );
                            }
                        }
                        .detach();
                    }

                    Ok(cx)
                })
                .await;
        }
        .detach();

        Ok(Fuso { accept_tx })
    }
}
