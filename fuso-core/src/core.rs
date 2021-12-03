use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    sync::Arc,
};

use async_trait::async_trait;
use futures::{lock::Mutex, AsyncWriteExt, TryStreamExt};
use smol::{
    channel::{unbounded, Receiver, Sender},
    future::FutureExt,
    lock::RwLock,
    net::TcpStream,
};

use fuso_api::{AsyncTcpSocketEx, FusoPacket, Result, Spwan};

use crate::retain::Heartbeat;
use crate::{dispatch::DynHandler, packet::Action};
use crate::{
    dispatch::{StrategyEx, TcpStreamRollback},
    retain::HeartGuard,
};

use crate::{
    dispatch::{Dispatch, Handler, State},
    handler::ChainHandler,
};

#[derive(Debug, Clone)]
pub struct Config {
    pub debug: bool,
    pub bind_addr: SocketAddr,
}

#[allow(unused)]
pub struct FusoStream<IO> {
    pub from: IO,
    pub to: IO,
}

pub struct Channel {
    pub conv: u64,
    pub core: HeartGuard<TcpStream>,
    pub config: Arc<Config>,
    pub strategys: Arc<Vec<Arc<Box<DynHandler<Arc<Channel>, Action>>>>>,
    pub wait_queue: Arc<Mutex<VecDeque<TcpStream>>>,
}

#[derive(Clone)]
pub struct Context {
    pub alloc_conv: Arc<Mutex<u64>>,
    pub config: Arc<Config>,
    pub handlers: Arc<Vec<Arc<Box<DynHandler<Arc<Self>, ()>>>>>,
    pub strategys: Arc<Vec<Arc<Box<DynHandler<Arc<Channel>, Action>>>>>,
    pub sessions: Arc<RwLock<HashMap<u64, Sender<TcpStream>>>>,
    pub accept_ax: Sender<FusoStream<TcpStream>>,
}

pub struct Fuso<IO> {
    accept_tx: Receiver<IO>,
}

pub struct FusoBuilder<C> {
    pub config: Option<Config>,
    pub handelrs: Vec<Arc<Box<DynHandler<C, ()>>>>,
    pub strategys: Vec<Arc<Box<DynHandler<Arc<Channel>, Action>>>>,
}

impl FusoBuilder<Arc<Context>> {
    #[inline]
    pub fn with_config(mut self, config: Config) -> Self {
        self.config = Some(config);
        self
    }

    #[inline]
    pub fn with_chain<F>(mut self, with_chain: F) -> Self
    where
        F: FnOnce(
            ChainHandler<TcpStreamRollback, Arc<Context>, fuso_api::Result<State<()>>>,
        )
            -> ChainHandler<TcpStreamRollback, Arc<Context>, fuso_api::Result<State<()>>>,
    {
        self.handelrs
            .push(Arc::new(Box::new(with_chain(ChainHandler::new()))));
        self
    }

    #[inline]
    pub fn chain_strategy<F>(mut self, with_chain: F) -> Self
    where
        F: FnOnce(
            ChainHandler<TcpStreamRollback, Arc<Channel>, fuso_api::Result<State<Action>>>,
        )
            -> ChainHandler<TcpStreamRollback, Arc<Channel>, fuso_api::Result<State<Action>>>,
    {
        self.strategys
            .push(Arc::new(Box::new(with_chain(ChainHandler::new()))));

        self
    }

    #[inline]
    pub fn chain_handler<H>(mut self, handler: H) -> Self
    where
        H: Handler<TcpStreamRollback, Arc<Context>, ()> + Send + Sync + 'static,
    {
        self.handelrs.push(Arc::new(Box::new(handler)));
        self
    }

    pub async fn build(self) -> fuso_api::Result<Fuso<FusoStream<TcpStream>>> {
        let config = Arc::new(self.config.unwrap());

        let (accept_ax, accept_tx) = unbounded();

        let bind_addr = config.bind_addr.clone();
        let listen = bind_addr.tcp_listen().await?;

        let handlers = Arc::new(self.handelrs);
        let strategys = Arc::new(self.strategys);

        let cx = Arc::new(Context {
            config,
            accept_ax,
            handlers,
            strategys,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            alloc_conv: Arc::new(Mutex::new(0)),
        });

        async move {
            log::info!("Service started successfully");
            log::info!("Bound to {}", bind_addr);
            log::info!("Waiting to connect");

            let _ = listen
                .incoming()
                .try_fold(cx, |cx, mut stream| async move {
                    log::debug!("[tcp] accept {}", stream.local_addr().unwrap());

                    {
                        let handlers = cx.handlers.clone();
                        let cx = cx.clone();
                        async move {
                            match stream.clone().dispatch(handlers, cx).await {
                                Ok(_) => {
                                    log::debug!(
                                        "[tcp] Successfully processed {}",
                                        stream.local_addr().unwrap()
                                    );
                                }
                                Err(_) => {
                                    let _ = stream.close().await;

                                    log::warn!(
                                        "[tcp] An illegal connection {}",
                                        stream.peer_addr().unwrap()
                                    );
                                }
                            };
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

impl Fuso<FusoStream<TcpStream>> {
    #[inline]
    pub fn builder() -> FusoBuilder<Arc<Context>> {
        FusoBuilder {
            config: None,
            handelrs: Vec::new(),
            strategys: Vec::new(),
        }
    }
}

#[async_trait]
impl<IO> fuso_api::FusoListener<IO> for Fuso<IO>
where
    IO: Send + Sync + 'static,
{
    #[inline]
    async fn accept(&mut self) -> Result<IO> {
        self.accept_tx.recv().await.map_err(|e| e.into())
    }

    #[inline]
    async fn close(&mut self) -> Result<()> {
        self.accept_tx.close();
        Ok(())
    }
}

impl Channel {
    #[inline]
    pub async fn try_wake(&self) -> Result<TcpStream> {
        match self.wait_queue.lock().await.pop_front() {
            Some(tcp) => Ok(tcp),
            None => Err("No task operation required".into()),
        }
    }

    #[inline]
    pub async fn suspend(&self, tcp: TcpStream) -> Result<()> {
        self.wait_queue.lock().await.push_back(tcp);
        Ok(())
    }
}

impl Context {
    #[inline]
    pub async fn fork(&self) -> (u64, Receiver<TcpStream>) {
        let (accept_tx, accept_ax) = unbounded();

        let mut sessions = self.sessions.write().await;

        let conv = loop {
            let (conv, _) = self.alloc_conv.lock().await.overflowing_add(1);

            match sessions.get(&conv) {
                Some(accept_tx) if accept_tx.is_closed() => {
                    break conv;
                }
                None => break conv,
                _ => {}
            }

            *self.alloc_conv.lock().await = conv;
        };

        sessions.insert(conv, accept_tx);

        (conv, accept_ax)
    }

    #[inline]
    pub async fn route(&self, conv: u64, tcp: TcpStream) -> Result<()> {
        let sessions = self.sessions.read().await;

        if let Some(accept_tx) = sessions.get(&conv) {
            if let Err(e) = accept_tx.send(tcp).await {
                Err(e.to_string().into())
            } else {
                Ok(())
            }
        } else {
            Err(format!("Session does not exist {}", conv).into())
        }
    }

    pub async fn spwan(&self, tcp: TcpStream, addr: Option<SocketAddr>) -> Result<u64> {
        let (conv, accept_ax) = self.fork().await;
        let accept_tx = self.accept_ax.clone();
        let clinet_addr = tcp.local_addr().unwrap();
        let bind_addr = addr.unwrap_or("0.0.0.0:0".parse().unwrap());
        let listen = bind_addr.tcp_listen().await?;
        let strategys = self.strategys.clone();

        let mut core = tcp.guard(5000).await?;
        let _ = core.send(Action::Accept(conv).into()).await?;

        let channel = Arc::new(Channel {
            conv,
            core,
            strategys,
            config: self.config.clone(),
            wait_queue: Arc::new(Mutex::new(VecDeque::new())),
        });

        log::info!(
            "New mapping {} -> {}",
            listen.local_addr().unwrap(),
            clinet_addr
        );

        async move {
            let accept_future = {
                let channel = channel.clone();
                async move {
                    loop {
                        let from = accept_ax.recv().await;

                        if from.is_err() {
                            log::warn!("An unavoidable error occurred {}", from.unwrap_err());
                            break;
                        }

                        let mut from = from.unwrap();
                        match channel.try_wake().await {
                            Ok(to) => {
                                if let Err(e) = accept_tx.send(FusoStream { from, to }).await {
                                    log::error!("An unavoidable error occurred {}", &e);
                                    break;
                                }
                            }
                            Err(e) => {
                                let _ = from.close().await;
                                log::warn!("{}", e);
                            }
                        }
                    }
                }
            };

            let fuso_future = {
                let mut io = channel.core.clone();
                async move {
                    loop {
                        match io.recv().await {
                            Ok(packet) => {
                                log::trace!("Client message received {:?}", packet);
                            }
                            Err(_) => {
                                log::warn!("Client session was aborted");
                                break;
                            }
                        }
                    }
                }
            };

            let future_listen = {
                async move {
                    let _ = listen
                        .incoming()
                        .try_fold(channel, |channel, mut tcp| async move {
                            {
                                log::debug!("connected {}", tcp.peer_addr().unwrap());

                                let channel = channel.clone();
                                async move {
                                    let mut core = channel.core.clone();

                                    let action = {
                                        tcp.clone()
                                            .select(channel.strategys.clone(), channel.clone())
                                            .await
                                    };

                                    match action {
                                        Ok(action) => {
                                            log::debug!("action {:?}", action);
                                            let _ = core.send(action.into()).await;
                                            let _ = channel.suspend(tcp).await;
                                        }
                                        Err(e) => {
                                            let _ = tcp.close().await;
                                            log::warn!(
                                                "Unable to process connection {} {}",
                                                e,
                                                tcp.peer_addr().unwrap()
                                            );
                                        }
                                    }
                                }
                                .detach();
                            }

                            Ok(channel)
                        })
                        .await;
                }
            };

            accept_future.race(fuso_future.race(future_listen)).await
        }
        .detach();

        Ok(conv)
    }
}
