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
use crate::ciphe::{Security, Xor};

use fuso_api::{
    AsyncTcpSocketEx, FusoPacket, Result, SafeStream, SafeStreamEx, Spwan, UdpListener,
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
    pub bind_addr: SocketAddr,
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
    pub core: HeartGuard<fuso_api::SafeStream<TcpStream>>,
    pub config: Arc<Config>,
    pub strategys: Arc<Vec<Arc<Box<DynHandler<Arc<Channel>, Action>>>>>,
    pub wait_queue: Arc<Mutex<HashMap<u64, SafeStream<TcpStream>>>>,
    pub udp_forward: Arc<Mutex<HashMap<u64, Sender<Vec<u8>>>>>,
}

pub struct Udp {
    id: u64,
    core: HeartGuard<fuso_api::SafeStream<TcpStream>>,
    forwards: Arc<Mutex<HashMap<u64, Sender<Vec<u8>>>>>,
}

#[derive(Clone)]
pub struct Context {
    pub alloc_conv: Arc<Mutex<u64>>,
    pub config: Arc<Config>,
    pub handlers: Arc<Vec<Arc<Box<DynHandler<Arc<Self>, ()>>>>>,
    pub strategys: Arc<Vec<Arc<Box<DynHandler<Arc<Channel>, Action>>>>>,
    pub sessions: Arc<RwLock<HashMap<u64, Sender<(u64, SafeStream<TcpStream>)>>>>,
    pub accept_ax: Sender<FusoStream<fuso_api::SafeStream<TcpStream>>>,
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
    pub fn chain_handler<F>(mut self, with_chain: F) -> Self
    where
        F: FnOnce(
            ChainHandler<SafeTcpStream, Arc<Context>, fuso_api::Result<State<()>>>,
        )
            -> ChainHandler<SafeTcpStream, Arc<Context>, fuso_api::Result<State<()>>>,
    {
        self.handelrs
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
    pub async fn try_wake(&self, id: &u64) -> Result<fuso_api::SafeStream<TcpStream>> {
        match self.wait_queue.lock().await.remove(id) {
            Some(tcp) => Ok(tcp),
            None => Err("No task operation required".into()),
        }
    }

    #[inline]
    pub async fn suspend(&self, id: u64, tcp: SafeStream<TcpStream>) -> Result<()> {
        self.wait_queue.lock().await.insert(id, tcp);
        Ok(())
    }

    #[inline]
    pub async fn gen_id(&self) -> u64 {
        let sessions = self.wait_queue.lock().await;

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
        let core = self.core.clone();
        let forwards = self.udp_forward.clone();
        let id = self.gen_id().await;

        forward(udp, Udp { id, core, forwards })
            .await
            .map_err(|e| e.into())
    }
}

impl Context {
    #[inline]
    pub async fn fork(&self) -> (u64, Receiver<(u64, SafeStream<TcpStream>)>) {
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
        id: u64,
        tcp: fuso_api::SafeStream<TcpStream>,
    ) -> Result<()> {
        let sessions = self.sessions.read().await;

        if let Some(accept_tx) = sessions.get(&conv) {
            if let Err(e) = accept_tx.send((id, tcp)).await {
                Err(e.to_string().into())
            } else {
                Ok(())
            }
        } else {
            Err(format!("Session does not exist {}", conv).into())
        }
    }

    pub async fn spwan(
        &self,
        tcp: fuso_api::SafeStream<TcpStream>,
        addr: Option<SocketAddr>,
        name: Option<String>,
    ) -> Result<u64> {
        let (conv, accept_ax) = self.fork().await;
        let accept_tx = self.accept_ax.clone();
        let clinet_addr = tcp.local_addr().unwrap();
        let bind_addr = addr.unwrap_or(([0, 0, 0, 0], 0).into());
        let listen = bind_addr.tcp_listen().await?;

        let mut core = tcp.guard(5000).await?;
        let _ = core.send(Action::Accept(conv).into()).await?;

        let strategys = self.strategys.clone();
        let name = name.unwrap_or("anonymous".to_string());

        let udp_forward = Arc::new(Mutex::new(HashMap::new()));

        let channel = Arc::new(Channel {
            conv,
            core,
            alloc_id: Arc::new(Mutex::new(0)),
            strategys,
            name: name.clone(),
            config: self.config.clone(),
            wait_queue: Arc::new(Mutex::new(HashMap::new())),
            udp_forward: udp_forward.clone(),
        });

        log::info!(
            "New mapping [{}] {} -> {}",
            name,
            listen.local_addr().unwrap(),
            clinet_addr
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

                        let (id, mut from) = from.unwrap();

                        let to = channel.try_wake(&id).await;

                        if to.is_err() {
                            let _ = from.close().await;
                            log::warn!("{}", to.unwrap_err());
                            continue;
                        }

                        let to = to.unwrap();

                        log::info!(
                            "{} -> {}",
                            from.peer_addr().unwrap(),
                            to.peer_addr().unwrap()
                        );

                        // 成功建立映射, 发给下一端处理
                        let err = accept_tx.send(FusoStream::new(from, to)).await;

                        if err.is_err() {
                            log::error!("An unavoidable error occurred {}", err.unwrap_err());
                            break;
                        }
                    }
                }
            };

            let fuso_future = {
                let mut core = channel.core.clone();
                // 服务端与客户端加密,
                // let mut core = core.ciphe(Xor::new(10)).await;

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
                            Action::UdpRespose(id, data) => {
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

            let future_listen = async move {
                let _ = listen
                    .incoming()
                    .try_fold(channel, |channel, tcp| async move {
                        {
                            log::info!("connected {}", tcp.peer_addr().unwrap());
                            let channel = channel.clone();
                            async move {
                                let mut tcp = tcp.as_safe_stream();
                                let mut core = channel.core.clone();

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

            accept_future.race(fuso_future.race(future_listen)).await
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

        self.forwards.lock().await.insert(self.id, sender);

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
