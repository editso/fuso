use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use futures::{AsyncWriteExt, Future, TryStreamExt};

use smol::{
    channel::{unbounded, Receiver, Sender},
    future::FutureExt,
    lock::{Mutex, RwLock},
    net::TcpStream,
    stream::StreamExt,
};

use fuso_api::{
    Advice, AsyncTcpSocketEx, Cipher, DynCipher, FusoAuth, FusoPacket, FusoStream, FusoStreamEx,
    Result, SafeStream, SafeStreamEx, Security, Spawn, UdpListener,
};

use crate::{dispatch::StrategyEx, retain::HeartGuard};

use crate::{
    dispatch::{DynHandler, SafeTcpStream},
    handsnake::Handsnake,
    packet::Action,
    FusoBuilder,
};

use crate::{packet::Addr, retain::Heartbeat};

use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct GlobalConfig {
    pub debug: bool,
    pub bind_addr: String,
}

#[allow(unused)]
pub struct FusoProxy<P> {
    pub cfg: Arc<Config>,
    pub(crate) session: Arc<Session>,
    pub proxy_dst: P,
    pub proxy_src: P,
}

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    /// 转发服务名称
    pub forward_name: Option<String>,
    /// 访问地址
    pub visit_addr: Option<String>,
    /// 桥接地址
    pub bridge_addr: Option<String>,
    /// socks5 password
    pub socks_passwd: Option<String>,
    /// 转发类型 all socks5
    pub forward_type: Option<String>,
    /// 加密类型
    pub crypt_type: Option<String>,
    /// 加密密钥
    pub crypt_secret: Option<String>,
}

pub struct Session {
    pub conv: u64,
    pub name: String,
    pub config: Arc<Config>,
    pub ciphers: Arc<
        HashMap<
            String,
            Arc<Box<dyn Fn(Option<String>) -> Result<Box<DynCipher>> + Send + Sync + 'static>>,
        >,
    >,
    pub alloc_id: Arc<Mutex<u64>>,
    pub tcp_core: HeartGuard<SafeStream<TcpStream>>,
    pub udp_core: Arc<Mutex<Option<HeartGuard<FusoStream>>>>,
    pub strategys: Arc<Vec<Arc<Box<DynHandler<Arc<Session>, Action>>>>>,
    pub global_config: Arc<GlobalConfig>,
    pub tcp_forward_map: Arc<Mutex<HashMap<u64, SafeStream<TcpStream>>>>,
    pub udp_forward_map: Arc<Mutex<HashMap<u64, Sender<Vec<u8>>>>>,
}

pub struct Udp {
    id: u64,
    core: FusoStream,
    forward_map: Arc<Mutex<HashMap<u64, Sender<Vec<u8>>>>>,
}

#[derive(Clone)]
pub struct Context {
    pub auth: Option<Arc<dyn FusoAuth<SafeTcpStream> + Send + Sync + 'static>>,
    pub config: Arc<GlobalConfig>,
    pub ciphers: Arc<
        HashMap<
            String,
            Arc<Box<dyn Fn(Option<String>) -> Result<Box<DynCipher>> + Send + Sync + 'static>>,
        >,
    >,
    pub sessions: Arc<RwLock<HashMap<u64, Sender<(Action, SafeStream<TcpStream>)>>>>,
    pub handlers: Arc<Vec<Arc<Box<DynHandler<Arc<Self>, ()>>>>>,
    pub strategys: Arc<Vec<Arc<Box<DynHandler<Arc<Session>, Action>>>>>,
    pub accept_ax: Sender<FusoProxy<SafeStream<TcpStream>>>,
    pub alloc_conv: Arc<Mutex<u64>>,
    pub handsnakes: Arc<Vec<Handsnake>>,
    pub advices: Vec<Arc<dyn Advice<SafeTcpStream, Box<DynCipher>> + Send + Sync + 'static>>,
}

pub struct Fuso<IO> {
    pub(crate) accept_tx: Receiver<IO>,
}

impl Fuso<FusoProxy<TcpStream>> {
    #[inline]
    pub fn builder() -> FusoBuilder<Arc<Context>> {
        FusoBuilder {
            auth: None,
            config: None,
            handlers: Vec::new(),
            strategys: Vec::new(),
            handsnakes: Vec::new(),
            ciphers: HashMap::new(),
            advices: Vec::new(),
        }
    }
}

impl GlobalConfig {
    pub fn new(bind_addr: &str) -> Self {
        Self {
            debug: false,
            bind_addr: bind_addr.into(),
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

impl Session {
    #[inline]
    pub async fn try_wake(&self, id: &u64) -> Result<fuso_api::SafeStream<TcpStream>> {
        match self.tcp_forward_map.lock().await.remove(id) {
            Some(tcp) => Ok(tcp),
            None => Err("No task operation required".into()),
        }
    }

    pub fn try_get_cipher(
        &self,
    ) -> fuso_api::Result<Option<Box<dyn Cipher + Send + Sync + 'static>>> {
        let cfg = self.config.as_ref();
        let cipher = self.ciphers.as_ref();

        if let Some(crypt_type) = cfg.crypt_type.as_ref() {
            if let Some(create_cipher) = cipher.get(crypt_type) {
                let secret = cfg.crypt_secret.clone();

                Ok(Some(create_cipher(secret)?))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    #[inline]
    pub async fn suspend(&self, id: u64, tcp: SafeStream<TcpStream>) -> Result<()> {
        self.tcp_forward_map.lock().await.insert(id, tcp);
        Ok(())
    }

    pub async fn bind_udp(&self, mut tcp: SafeStream<TcpStream>) -> Result<()> {
        tcp.send(Action::Accept(self.conv).into()).await?;

        let cipher = self.try_get_cipher()?;

        log::info!("[bind_udp] bind udp");

        let tcp = if let Some(cipher) = cipher {
            let tcp = tcp.cipher(cipher);
            tcp.as_fuso_stream()
        } else {
            tcp.as_fuso_stream()
        };

        let bind_udp = tcp.guard(5000).await?;

        let mut udp_core = self.udp_core.lock().await;

        if let Some(mut udp_core) = udp_core.take() {
            let _ = udp_core.close().await;
        }

        {
            let mut udp_core = bind_udp.clone();
            let forward_map = self.udp_forward_map.clone();
            let locked_udp_core = self.udp_core.clone();

            async move {
                loop {
                    match udp_core.recv().await {
                        Ok(packet) => {
                            let action: Result<Action> = packet.try_into();

                            if action.is_err() {
                                log::warn!("bad packet {}", action.unwrap_err());
                                break;
                            }

                            let action = action.unwrap();

                            match action {
                                Action::UdpResponse(id, packet) => {
                                    if let Some(sender) = forward_map.lock().await.remove(&id) {
                                        let _ = sender.send(packet).await;
                                    }
                                }
                                _ => {}
                            }
                        }
                        Err(e) => {
                            log::warn!("udp_forward {}", e);
                            break;
                        }
                    }
                }
                if let Some(mut udp_core) = locked_udp_core.lock().await.take() {
                    let _ = udp_core.close().await;
                    log::warn!("Udp forwarding is turned off");
                }
            }
        }
        .detach();

        *udp_core = Some(bind_udp);

        Ok(())
    }

    #[inline]
    pub async fn gen_id(&self) -> u64 {
        let sessions = self.tcp_forward_map.lock().await;

        loop {
            let (conv, _) = self.alloc_id.lock().await.overflowing_add(1);

            if sessions.get(&conv).is_some() {
                continue;
            }

            *self.alloc_id.lock().await = conv;

            break conv;
        }
    }

    pub fn test(&self, name: &str) -> bool {
        if let Some(t) = self.config.forward_type.as_ref() {
            t.eq("all") || name.eq(t)
        } else {
            true
        }
    }

    #[inline]
    pub async fn udp_forward<F, Fut>(&self, forward: F) -> fuso_api::Result<()>
    where
        F: FnOnce(UdpListener, Udp) -> Fut,
        Fut: Future<Output = std::io::Result<()>>,
    {
        let udp = UdpListener::bind("0.0.0.0:0").await?;

        let core = self.udp_core.lock().await.as_ref().map_or(
            {
                let tcp = self.tcp_core.clone();
                tcp.as_fuso_stream()
            },
            |udp_core| {
                let udp = udp_core.clone();
                udp.as_fuso_stream()
            },
        );

        let forwards = self.udp_forward_map.clone();
        let id = self.gen_id().await;

        forward(
            udp,
            Udp {
                id,
                core,
                forward_map: forwards,
            },
        )
        .await
        .map_err(|e| e.into())
    }
}

impl Context {
    #[inline]
    pub fn get_auth(&self) -> Option<Arc<dyn FusoAuth<SafeTcpStream> + Send + Sync + 'static>> {
        self.auth.clone()
    }

    #[inline]
    pub async fn fork(&self) -> (u64, Receiver<(Action, SafeStream<TcpStream>)>) {
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
        };

        *self.alloc_conv.lock().await = conv;

        sessions.insert(conv, accept_tx);

        (conv, accept_ax)
    }

    #[inline]
    pub async fn route(
        &self,
        conv: u64,
        action: Action,
        tcp: fuso_api::SafeStream<TcpStream>,
    ) -> Result<()> {
        let sessions = self.sessions.read().await;

        if let Some(accept_tx) = sessions.get(&conv) {
            if let Err(e) = accept_tx.send((action, tcp)).await {
                Err(e.to_string().into())
            } else {
                Ok(())
            }
        } else {
            Err(format!("Session does not exist {}", conv).into())
        }
    }

    pub async fn spawn(
        &self,
        tcp: fuso_api::SafeStream<TcpStream>,
        config: Option<String>,
    ) -> Result<u64> {
        let (conv, accept_ax) = self.fork().await;
        let accept_tx = self.accept_ax.clone();
        let client_addr = tcp.local_addr().unwrap();

        let cfg = Arc::new({
            config.map_or(Ok(Config::default()), |config| {
                serde_json::from_str(&config).map_err(|e| fuso_api::Error::from(e.to_string()))
            })?
        });

        log::info!("{:#?}", cfg);

        let visit_addr = cfg.visit_addr.clone().unwrap_or(String::from("0.0.0.0:0"));
        let listen = visit_addr.tcp_listen().await?;

        let mut core = tcp.guard(5000).await?;
        let _ = core.send(Action::Accept(conv).into()).await?;

        let strategys = self.strategys.clone();
        let name = cfg.forward_name.clone().unwrap_or("anonymous".to_string());

        let udp_forward = Arc::new(Mutex::new(HashMap::new()));

        let session = Arc::new(Session {
            conv,
            strategys,
            tcp_core: core,
            name: name.clone(),
            config: cfg.clone(),
            alloc_id: Arc::new(Mutex::new(0)),
            global_config: self.config.clone(),
            tcp_forward_map: Arc::new(Mutex::new(HashMap::new())),
            udp_forward_map: udp_forward.clone(),
            udp_core: Arc::new(Mutex::new(None)),
            ciphers: self.ciphers.clone(),
        });

        log::info!(
            "New mapping [{}] {} -> {}",
            name,
            listen.local_addr().unwrap(),
            client_addr
        );

        async move {
            let accept_future = {
                let session = session.clone();
                async move {
                    loop {
                        // 接收客户端发来的连接
                        let proxy_dst = accept_ax.recv().await;

                        if proxy_dst.is_err() {
                            log::warn!("An unavoidable error occurred");
                            break;
                        }

                        let (action, proxy_dst) = proxy_dst.unwrap();

                        match action {
                            Action::Connect(_, id) => {
                                if let Ok(proxy_src) = session.try_wake(&id).await {
                                    let proxy = FusoProxy {
                                        proxy_src,
                                        proxy_dst,
                                        session: session.clone(),
                                        cfg: cfg.clone(),
                                    };

                                    let state = accept_tx.send(proxy).await;

                                    if state.is_err() {
                                        log::error!(
                                            "An unavoidable error occurred {}",
                                            state.unwrap_err()
                                        );
                                        break;
                                    }
                                }
                            }
                            Action::UdpBind(_) => {
                                let addr = proxy_dst.peer_addr().unwrap();
                                if let Err(e) = session.bind_udp(proxy_dst).await {
                                    log::warn!("Failed to bind udp forwarding {} {}", addr, e);
                                } else {
                                    log::info!("Binding udp forwarding successfully {}", addr)
                                }
                            }
                            _ => {}
                        };
                    }
                }
            };

            let fuso_future = {
                let mut core = session.tcp_core.clone();
                // 服务端与客户端加密,
                // let mut core = core.cipher(Xor::new(10)).await;

                async move {
                    loop {
                        // 接收客户端发来的包
                        let packet = core.recv().await;

                        if packet.is_err() {
                            log::warn!("Client session was aborted {}", name);
                            break;
                        }

                        let packet: Result<Action> = packet.unwrap().try_into();

                        log::trace!("Client message received {:?}", packet);

                        if packet.is_err() {
                            log::warn!("recv bad packet");
                        }

                        let action = packet.unwrap();

                        match action {
                            Action::UdpResponse(id, data) => {
                                if let Some(sender) = udp_forward.lock().await.remove(&id) {
                                    if let Err(e) = sender.send(data).await {
                                        log::warn!("udp_forward {}", e);
                                    };
                                }
                            }
                            _ => {
                                // 暂时忽略其他包
                            }
                        }
                    }
                }
            };

            let listen_future = async move {
                let _ = listen
                    .incoming()
                    .try_fold(session, |channel, tcp| async move {
                        {
                            log::debug!("connected {}", tcp.peer_addr().unwrap());
                            let channel = channel.clone();
                            async move {
                                let mut tcp = tcp.as_safe_stream();
                                let mut core = channel.tcp_core.clone();

                                let action = {
                                    let strategys = channel.strategys.clone();
                                    let tcp = tcp.clone();
                                    // 选择一个合适的策略
                                    tcp.select(strategys, channel.clone()).await
                                };

                                if action.is_err() {
                                    let _ = tcp.close().await;
                                    log::warn!(
                                        "Unable to process connection {} {}",
                                        action.unwrap_err(),
                                        tcp.peer_addr().map_or_else(
                                            |_| { String::from("Unknown addr") },
                                            |addr| addr.to_string()
                                        )
                                    );
                                } else {
                                    let action = action.unwrap();
                                    log::debug!("action {:?}", action);

                                    match action {
                                        Action::Forward(id, addr) => {
                                            let id = {
                                                if id.eq(&0) {
                                                    channel.gen_id().await
                                                } else {
                                                    id
                                                }
                                            };

                                            let _ = channel.suspend(id, tcp).await;

                                            // 通知客户端进行转发
                                            let _ =
                                                core.send(Action::Forward(id, addr).into()).await;
                                        }
                                        _ => {
                                            log::debug!("{:?}", action)
                                        }
                                    }
                                }
                            }
                            .detach();
                        }

                        Ok(channel)
                    })
                    .await;
            };

            accept_future.race(fuso_future.race(listen_future)).await
        }
        .detach();

        Ok(conv)
    }
}

impl<T> FusoProxy<T> {
    #[inline]
    pub fn split(self) -> (T, T, Arc<Config>) {
        (self.proxy_src, self.proxy_dst, self.cfg)
    }

    pub fn try_get_cipher(
        &self,
    ) -> fuso_api::Result<Option<Box<dyn Cipher + Send + Sync + 'static>>> {
        self.session.try_get_cipher()
    }
}

impl<T> futures::Stream for Fuso<T> {
    type Item = T;

    #[inline]
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.accept_tx.poll_next(cx)
    }
}

impl Udp {
    pub async fn call(&mut self, addr: Addr, packet: &[u8]) -> std::io::Result<Vec<u8>> {
        let (sender, receiver) = unbounded();

        self.forward_map.lock().await.insert(self.id, sender);

        self.core
            .send(Action::UdpRequest(self.id, addr, packet.to_vec()).into())
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        receiver
            .recv()
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    }
}
