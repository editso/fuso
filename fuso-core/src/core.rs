use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use futures::{lock::Mutex, AsyncWriteExt, Future, TryStreamExt};
use smol::{
    channel::{unbounded, Receiver, Sender},
    future::FutureExt,
    lock::RwLock,
    net::TcpStream,
    stream::StreamExt,
};

#[allow(unused)]
use crate::cipher::{Security, Xor};

use fuso_api::{
    AsyncTcpSocketEx, FusoPacket, Result, SafeStream, SafeStreamEx, Spawn, UdpListener,
};

use crate::{dispatch::DynHandler, packet::Action};
use crate::{
    dispatch::{SafeTcpStream, StrategyEx},
    retain::HeartGuard,
};
use crate::{packet::Addr, retain::Heartbeat};

use crate::{
    dispatch::{Dispatch, State},
    handler::ChainHandler,
};

#[derive(Debug, Clone)]
pub struct Config {
    pub debug: bool,
    pub bind_addr: String,
}

#[allow(unused)]
pub struct FusoStream<IO> {
    pub from: IO,
    pub to: IO,
}

pub struct Channel {
    pub conv: u64,
    pub alloc_id: Arc<Mutex<u64>>,
    pub name: String,
    pub config: Arc<Config>,
    pub tcp_core: HeartGuard<fuso_api::SafeStream<TcpStream>>,
    pub udp_core: Arc<Mutex<Option<HeartGuard<fuso_api::SafeStream<TcpStream>>>>>,
    pub strategys: Arc<Vec<Arc<Box<DynHandler<Arc<Channel>, Action>>>>>,
    pub tcp_forward_map: Arc<Mutex<HashMap<u64, SafeStream<TcpStream>>>>,
    pub udp_forward_map: Arc<Mutex<HashMap<u64, Sender<Vec<u8>>>>>,
}

pub struct Udp {
    id: u64,
    core: HeartGuard<fuso_api::SafeStream<TcpStream>>,
    forward_map: Arc<Mutex<HashMap<u64, Sender<Vec<u8>>>>>,
}

#[derive(Clone)]
pub struct Context {
    pub alloc_conv: Arc<Mutex<u64>>,
    pub config: Arc<Config>,
    pub handlers: Arc<Vec<Arc<Box<DynHandler<Arc<Self>, ()>>>>>,
    pub strategys: Arc<Vec<Arc<Box<DynHandler<Arc<Channel>, Action>>>>>,
    pub sessions: Arc<RwLock<HashMap<u64, Sender<(Action, SafeStream<TcpStream>)>>>>,
    pub accept_ax: Sender<FusoStream<fuso_api::SafeStream<TcpStream>>>,
}

pub struct Fuso<IO> {
    accept_tx: Receiver<IO>,
}

pub struct FusoBuilder<C> {
    pub config: Option<Config>,
    pub handlers: Vec<Arc<Box<DynHandler<C, ()>>>>,
    pub strategys: Vec<Arc<Box<DynHandler<Arc<Channel>, Action>>>>,
}

impl FusoBuilder<Arc<Context>> {
    #[inline]
    pub fn with_config(mut self, config: Config) -> Self {
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
            ChainHandler<SafeTcpStream, Arc<Channel>, fuso_api::Result<State<Action>>>,
        )
            -> ChainHandler<SafeTcpStream, Arc<Channel>, fuso_api::Result<State<Action>>>,
    {
        self.strategys
            .push(Arc::new(Box::new(with_chain(ChainHandler::new()))));

        self
    }

    pub async fn build(
        self,
    ) -> fuso_api::Result<Fuso<FusoStream<fuso_api::SafeStream<TcpStream>>>> {
        let config = Arc::new(self.config.unwrap());

        let (accept_ax, accept_tx) = unbounded();

        let bind_addr = config.bind_addr.clone();

        let listen = {
            let bind_addr = bind_addr.clone();
            bind_addr.tcp_listen().await?
        };

        let handlers = Arc::new(self.handlers);
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
            log::info!("Bind to {}", bind_addr);
            log::info!("Waiting to connect");

            let _ = listen
                .incoming()
                .try_fold(cx, |cx, mut tcp| async move {
                    log::debug!("[tcp] accept {}", tcp.local_addr().unwrap());

                    {
                        let handlers = cx.handlers.clone();
                        let cx = cx.clone();
                        async move {
                            let process = tcp.clone().dispatch(handlers, cx).await;

                            if process.is_err() {
                                let _ = tcp.close().await;

                                log::warn!(
                                    "[tcp] An illegal connection {}",
                                    tcp.peer_addr().unwrap()
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

impl Fuso<FusoStream<TcpStream>> {
    #[inline]
    pub fn builder() -> FusoBuilder<Arc<Context>> {
        FusoBuilder {
            config: None,
            handlers: Vec::new(),
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
    pub async fn try_wake(&self, id: &u64) -> Result<fuso_api::SafeStream<TcpStream>> {
        match self.tcp_forward_map.lock().await.remove(id) {
            Some(tcp) => Ok(tcp),
            None => Err("No task operation required".into()),
        }
    }

    #[inline]
    pub async fn suspend(&self, id: u64, tcp: SafeStream<TcpStream>) -> Result<()> {
        self.tcp_forward_map.lock().await.insert(id, tcp);
        Ok(())
    }

    pub async fn bind_udp(&self, mut tcp: SafeStream<TcpStream>) -> Result<()> {
        tcp.send(Action::Accept(self.conv).into()).await?;

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
                            log::warn!("{}", e);
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

    #[inline]
    pub async fn udp_forward<F, Fut>(&self, forward: F) -> fuso_api::Result<()>
    where
        F: FnOnce(UdpListener, Udp) -> Fut,
        Fut: Future<Output = std::io::Result<()>>,
    {
        let udp = UdpListener::bind("0.0.0.0:0").await?;

        let core = self
            .udp_core
            .lock()
            .await
            .as_ref()
            .map_or(self.tcp_core.clone(), |udp_core| udp_core.clone());

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
        addr: Option<SocketAddr>,
        name: Option<String>,
    ) -> Result<u64> {
        let (conv, accept_ax) = self.fork().await;
        let accept_tx = self.accept_ax.clone();
        let client_addr = tcp.local_addr().unwrap();
        let bind_addr = addr.unwrap_or(([0, 0, 0, 0], 0).into());
        let listen = bind_addr.tcp_listen().await?;

        let mut core = tcp.guard(5000).await?;
        let _ = core.send(Action::Accept(conv).into()).await?;

        let strategys = self.strategys.clone();
        let name = name.unwrap_or("anonymous".to_string());

        let udp_forward = Arc::new(Mutex::new(HashMap::new()));

        let channel = Arc::new(Channel {
            conv,
            tcp_core: core,
            alloc_id: Arc::new(Mutex::new(0)),
            strategys,
            name: name.clone(),
            config: self.config.clone(),
            tcp_forward_map: Arc::new(Mutex::new(HashMap::new())),
            udp_forward_map: udp_forward.clone(),
            udp_core: Arc::new(Mutex::new(None)),
        });

        log::info!(
            "New mapping [{}] {} -> {}",
            name,
            listen.local_addr().unwrap(),
            client_addr
        );

        async move {
            let accept_future = {
                let channel = channel.clone();
                async move {
                    loop {
                        // 接收客户端发来的连接
                        let from = accept_ax.recv().await;

                        if from.is_err() {
                            log::warn!("An unavoidable error occurred {}", from.unwrap_err());
                            break;
                        }

                        let (action, mut from) = from.unwrap();

                        match action {
                            Action::Connect(_, id) => {
                                if let Ok(to) = channel.try_wake(&id).await {
                                    let state =
                                        accept_tx.send(FusoStream::new(from.clone(), to)).await;
                                    if state.is_err() {
                                        let _ = from.close().await;
                                        log::error!(
                                            "An unavoidable error occurred {}",
                                            state.unwrap_err()
                                        );
                                    }
                                }
                            }
                            Action::UdpBind(_) => {
                                let addr = from.peer_addr().unwrap();
                                if let Err(e) = channel.bind_udp(from).await {
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
                let mut core = channel.tcp_core.clone();
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
                                        log::warn!("{}", e);
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
                    .try_fold(channel, |channel, tcp| async move {
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
                                        tcp.peer_addr().unwrap()
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

                                            // 通知客户端需执行的方法
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

impl<T> FusoStream<T> {
    #[inline]
    pub fn new(from: T, to: T) -> Self {
        Self { from, to }
    }

    #[inline]
    pub fn split(self) -> (T, T) {
        (self.from, self.to)
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
