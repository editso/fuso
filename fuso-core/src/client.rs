use std::{net::SocketAddr, sync::Arc};

use fuso_api::{
    async_trait, Advice, AsyncTcpSocketEx, DynCipher, Error, Forward, FusoListener, FusoPacket,
    FusoStreamEx, Result, Security, SecurityEx, Spawn,
};

use futures::{AsyncRead, AsyncWrite};
use smol::{
    channel::{unbounded, Receiver, Sender},
    net::{TcpStream, UdpSocket},
    stream::StreamExt,
};

use crate::{
    bridge::Bridge,
    handsnake::HandsnakeEx,
    packet::{Action, Addr},
};

use crate::{
    handsnake::Handsnake,
    retain::{HeartGuard, Heartbeat},
};

#[allow(unused)]
pub struct FusoProxy {
    conv: u64,
    action: Action,
    addr: String,
    context: Arc<Context>,
}

pub struct Fuso {
    accept_ax: Receiver<FusoProxy>,
}

pub struct Builder {
    config: Option<Config>,
    advices: Vec<Arc<dyn Advice<TcpStream, Box<DynCipher>> + Send + Sync + 'static>>,
    cipher: Option<
        Box<
            dyn Fn(Option<String>) -> fuso_api::Result<Option<Box<DynCipher>>>
                + Send
                + Sync
                + 'static,
        >,
    >,
}

#[derive(Debug, Clone)]
pub struct Config {
    // 名称, 可选的
    pub forward_name: Option<String>,
    // 服务端地址
    pub server_addr: String,
    // 自定义服务绑定的端口, 如果不指定默认为随机分配
    pub visit_port: Option<u16>,
    // 桥接监听的地址
    pub bridge_addr: Option<String>,
    // 转发地址
    pub forward_addr: String,
    // 首次建立连接发送的头部信息
    pub handsnake: Option<Handsnake>,
    // sock5代理密码
    pub socks_passwd: Option<String>,
    // forward type
    pub forward_type: Option<String>,
    // 传输加密类型
    pub crypt_type: Option<String>,
    // 传输加密密钥
    pub crypt_secret: Option<String>,
}

#[derive(Clone)]
pub struct Context {
    pub(crate) advices: Vec<Arc<dyn Advice<TcpStream, Box<DynCipher>> + Send + Sync + 'static>>,
    pub(crate) config: Arc<Config>,
    pub(crate) cipher: Arc<
        Option<
            Box<
                dyn Fn(Option<String>) -> fuso_api::Result<Option<Box<DynCipher>>>
                    + Send
                    + Sync
                    + 'static,
            >,
        >,
    >,
}

impl Context {
    pub fn get_cipher(&self) -> fuso_api::Result<Option<Box<DynCipher>>> {
        let secret = self.config.crypt_secret.clone();
        if let Some(create_cipher) = self.cipher.as_ref() {
            create_cipher(secret)
        } else {
            Ok(None)
        }
    }
}

impl Builder {
    pub fn add_advice<A>(mut self, advice: A) -> Self
    where
        A: Advice<TcpStream, Box<DynCipher>> + Send + Sync + 'static,
    {
        self.advices.push(Arc::new(advice));
        self
    }

    pub fn use_config(mut self, config: Config) -> Self {
        self.config = Some(config);
        self
    }

    pub fn set_cipher<F>(mut self, cipher: F) -> Self
    where
        F: Fn(Option<String>) -> fuso_api::Result<Option<Box<DynCipher>>> + Send + Sync + 'static,
    {
        let cipher = Box::new(cipher);
        self.cipher = Some(cipher);
        self
    }

    pub async fn build(self) -> fuso_api::Result<Fuso> {
        let cfg = Arc::new(self.config.expect("config required"));

        let cx = Context {
            advices: self.advices,
            config: cfg.clone(),
            cipher: Arc::new(self.cipher),
        };

        let cx = Arc::new(cx);

        let server_addr = cfg.server_addr.clone();
        let visit_addr = cfg.visit_port.clone();
        let bridge_addr = cfg.bridge_addr.clone();
        let handsnake = cfg.handsnake.clone();

        let mut stream = server_addr.clone().tcp_connect().await?;

        let server_socket_addr = stream.peer_addr().unwrap();

        if let Some(handsnake) = handsnake.as_ref() {
            stream.write_handsnake(handsnake).await?
        }

        let visit_addr: Option<SocketAddr> =
            visit_addr.map_or(None, |port| Some(([0, 0, 0, 0], port).into()));

        let cipher = stream.select_cipher(&cx.advices).await?;

        let mut stream = if let Some(cipher) = cipher {
            let cipher = stream.cipher(cipher);
            cipher.as_fuso_stream()
        } else {
            stream.as_fuso_stream()
        };

        let json = serde_json::json!({
            "forward_name": cfg.forward_name,
            "visit_addr": visit_addr,
            "bridge_addr": bridge_addr,
            "socks_passwd": cfg.socks_passwd,
            "forward_type": cfg.forward_type,
            "crypt_type": cfg.crypt_type,
            "crypt_secret": cfg.crypt_secret
        })
        .to_string();

        stream.send(Action::TcpBind(Some(json)).into()).await?;

        let action: Action = stream.recv().await?.try_into()?;

        let (accept_tx, accept_ax) = unbounded();

        match action {
            Action::Accept(conv) => {
                log::info!("Service binding is successful {}", conv);

                // let server_addr = stream.peer_addr()?;
                let server_socket_addr = server_socket_addr.clone();
                let server_addr = server_addr.clone();

                async move {
                    let race_future = smol::future::race(
                        Fuso::run_core(conv, cx.clone(), stream, accept_tx, server_addr.clone()),
                        Fuso::run_udp_forward(conv, server_addr, cx.clone()),
                    );

                    let bridge_future = async move {
                        if bridge_addr.is_none() {
                            smol::future::pending::<()>().await;
                        } else {
                            let _ =
                                Fuso::run_bridge(bridge_addr.unwrap(), server_socket_addr).await;
                        }
                        Ok(())
                    };

                    let _ = smol::future::race(race_future, bridge_future).await;
                }
                .detach();
            }
            Action::Err(e) => {
                log::error!("Server error message {}", e);
                panic!()
            }
            _ => {}
        }

        Ok(Fuso { accept_ax })
    }
}

impl Fuso {
    pub fn builder() -> Builder {
        Builder {
            advices: Vec::new(),
            config: None,
            cipher: None,
        }
    }

    async fn udp_forward<S>(
        mut core: HeartGuard<S>,
        id: u64,
        addr: Addr,
        packet: Vec<u8>,
    ) -> std::io::Result<()>
    where
        S: AsyncRead + AsyncWrite + Clone + Unpin + Send + Sync + 'static,
    {
        let udp = UdpSocket::bind("0.0.0.0:0").await?;

        let addr = match addr {
            Addr::Socket(addr) => addr.to_string(),
            Addr::Domain(domain, port) => {
                format!("{}:{}", domain, port)
            }
        };

        log::info!("[udp_forward] {}", addr);

        udp.connect(addr).await?;

        udp.send(&packet).await?;

        let mut buf = Vec::new();

        buf.resize(1500, 0);

        let n = udp.recv(&mut buf).await?;

        buf.truncate(n);

        let _ = core.send(Action::UdpResponse(id, buf).into()).await;

        Ok(())
    }

    async fn run_udp_forward(conv: u64, server_addr: String, cx: Arc<Context>) -> Result<()> {
        let mut udp_bind = TcpStream::connect(server_addr).await?;
        let cfg = cx.config.clone();

        if let Some(handsnake) = cfg.handsnake.as_ref() {
            udp_bind.write_handsnake(handsnake).await?;
        }

        let cipher = cx.get_cipher()?;

        udp_bind.send(Action::UdpBind(conv).into()).await?;

        let action: Action = udp_bind.recv().await?.try_into()?;

        match action {
            Action::Accept(_) => {
                let udp_bind = if cipher.is_some() {
                    udp_bind.cipher(cipher.unwrap()).as_fuso_stream()
                } else {
                    udp_bind.as_fuso_stream()
                };

                let mut udp_bind = udp_bind.guard(5000).await?;

                loop {
                    let action: Action = udp_bind.recv().await?.try_into()?;
                    // 只处理udp转发包
                    match action {
                        Action::Ping => {}
                        Action::UdpRequest(id, addr, packet) => {
                            log::trace!("udp forward {} addr={:?}, packet={:?}", id, addr, packet);
                            Self::udp_forward(udp_bind.clone(), id, addr, packet).detach()
                        }
                        _ => {
                            log::warn!("Not a udp forwarding packet {:?}", action);
                        }
                    }
                }
            }
            _ => smol::future::pending().await,
        }
    }

    async fn run_core<S>(
        conv: u64,
        context: Arc<Context>,
        core: S,
        accept_tx: Sender<FusoProxy>,
        server_addr: String,
    ) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Clone + Unpin + Send + Sync + 'static,
    {
        let mut core = core.guard(5000).await?;

        loop {
            let action: Action = core.recv().await?.try_into()?;

            match action {
                Action::Ping => {}
                Action::UdpRequest(id, addr, packet) => {
                    log::trace!("udp forward {} addr={:?}, packet={:?}", id, addr, packet);

                    Self::udp_forward(core.clone(), id, addr, packet).detach()
                }
                action => {
                    let proxy = FusoProxy {
                        conv,
                        action,
                        addr: server_addr.clone(),
                        context: context.clone(),
                    };

                    accept_tx.send(proxy).await.map_err(|e| {
                        let err: fuso_api::Error = e.to_string().into();
                        err
                    })?;
                }
            }
        }
    }

    async fn run_bridge(bridge_bind_addr: String, server_connect_addr: SocketAddr) -> Result<()> {
        let mut core = Bridge::bind(bridge_bind_addr, server_connect_addr).await?;

        log::info!("[bridge] Bridge service opened successfully");

        loop {
            match core.accept().await {
                Ok((from, to)) => {
                    log::info!(
                        "Bridge to {} -> {}",
                        from.peer_addr().unwrap(),
                        to.peer_addr().unwrap()
                    );

                    from.forward(to).detach();
                }
                Err(_) => {
                    break;
                }
            }
        }

        Ok(())
    }
}

impl FusoProxy {
    #[inline]
    pub async fn join(self) -> Result<(TcpStream, TcpStream, Arc<Context>)> {
        let config = self.context.config.clone();
        let (id, addr) = {
            match self.action {
                Action::Forward(id, Addr::Domain(domain, port)) => {
                    log::info!("[domain] addr={}:{}", domain, port);

                    (id, format!("{}:{}", domain, port))
                }
                Action::Forward(id, Addr::Socket(addr)) if addr.port() == 0 => {
                    let addr = config.forward_addr.clone();
                    log::info!("[forward_local] addr={}", addr);

                    (id, addr)
                }
                Action::Forward(id, Addr::Socket(addr)) => {
                    log::info!("[addr] addr={}", addr);

                    (id, addr.to_string())
                }
                action => {
                    return Err(format!("Unsupported operation {:?}", action).into());
                }
            }
        };

        let to = TcpStream::connect(addr).await?;

        let mut stream = TcpStream::connect(self.addr)
            .await
            .map_err(|e| Error::with_io(e))?;

        if let Some(handsnake) = config.handsnake.as_ref() {
            stream.write_handsnake(handsnake).await?;
        }

        stream.send(Action::Connect(self.conv, id).into()).await?;

        Ok((stream, to, self.context))
    }
}

#[async_trait]
impl FusoListener<FusoProxy> for Fuso {
    #[inline]
    async fn accept(&mut self) -> Result<FusoProxy> {
        Ok(self.accept_ax.recv().await.map_err(|e| {
            Error::with_io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?)
    }

    #[inline]
    async fn close(&mut self) -> Result<()> {
        self.accept_ax.close();
        Ok(())
    }
}

impl futures::Stream for Fuso {
    type Item = FusoProxy;

    #[inline]
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.accept_ax.poll_next(cx)
    }
}
