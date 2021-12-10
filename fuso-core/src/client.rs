use std::{net::SocketAddr, sync::Arc};

use fuso_api::{
    async_trait, AsyncTcpSocketEx, Error, Forward, FusoListener, FusoPacket, Result, Spawn,
};

use smol::{
    channel::{unbounded, Receiver, Sender},
    net::{TcpStream, UdpSocket},
    stream::StreamExt,
};

use crate::retain::{HeartGuard, Heartbeat};
use crate::{
    bridge::Bridge,
    packet::{Action, Addr},
};

#[allow(unused)]
#[derive(Debug)]
pub struct Reactor {
    conv: u64,
    action: Action,
    addr: SocketAddr,
    config: Arc<Config>,
}

pub struct Fuso {
    accept_ax: Receiver<Reactor>,
}

#[derive(Debug)]
pub struct Config {
    // 名称, 可选的
    pub name: Option<String>,
    // 服务端地址
    pub server_addr: SocketAddr,
    // 自定义服务绑定的端口, 如果不指定默认为随机分配
    pub server_bind_port: u16,
    // 桥接监听的地址
    pub bridge_addr: Option<SocketAddr>,
    // 转发地址
    pub forward_addr: String,
}

impl Fuso {
    async fn udp_forward(
        mut core: HeartGuard<TcpStream>,
        id: u64,
        addr: Addr,
        packet: Vec<u8>,
    ) -> std::io::Result<()> {
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

    async fn run_udp_forward(conv: u64, server_addr: SocketAddr) -> Result<()> {
        let mut udp_bind = TcpStream::connect(server_addr).await?;

        udp_bind.send(Action::UdpBind(conv).into()).await?;

        let action: Action = udp_bind.recv().await?.try_into()?;

        match action {
            Action::Accept(_) => {
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

    async fn run_core(
        conv: u64,
        config: Arc<Config>,
        core: TcpStream,
        accept_tx: Sender<Reactor>,
    ) -> Result<()> {
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
                    let reactor = Reactor {
                        conv,
                        action,
                        addr: core.peer_addr().unwrap(),
                        config: config.clone(),
                    };

                    accept_tx.send(reactor).await.map_err(|e| {
                        let err: fuso_api::Error = e.to_string().into();
                        err
                    })?;
                }
            }
        }
    }

    async fn run_bridge(
        bridge_bind_addr: SocketAddr,
        server_connect_addr: SocketAddr,
    ) -> Result<()> {
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

    pub async fn bind(config: Config) -> Result<Self> {
        let cfg = Arc::new(config);
        let server_addr = cfg.server_addr.clone();
        let server_bind_port = cfg.server_bind_port.clone();
        let bridge_addr = cfg.bridge_addr.clone();
        let name = cfg.name.clone();

        let mut stream = server_addr.tcp_connect().await?;

        log::debug!("{}", server_bind_port);

        let bind_addr = server_bind_port
            .ne(&0)
            .then(|| SocketAddr::from(([0, 0, 0, 0], server_bind_port)));

        stream.send(Action::TcpBind(name, bind_addr).into()).await?;

        let action: Action = stream.recv().await?.try_into()?;

        let (accept_tx, accept_ax) = unbounded();

        match action {
            Action::Accept(conv) => {
                log::debug!("Service binding is successful {}", conv);

                let server_addr = stream.peer_addr()?;

                async move {
                    let race_future = smol::future::race(
                        Self::run_core(conv, cfg, stream, accept_tx),
                        Self::run_udp_forward(conv, server_addr),
                    );

                    let bridge_future = async move {
                        if bridge_addr.is_none() {
                            smol::future::pending::<()>().await;
                        } else {
                            let _ = Self::run_bridge(bridge_addr.unwrap(), server_addr).await;
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

        Ok(Self { accept_ax })
    }
}

impl Reactor {
    #[inline]
    pub async fn join(self) -> Result<(TcpStream, TcpStream)> {
        let (id, addr) = {
            match self.action {
                Action::Forward(id, Addr::Domain(domain, port)) => {
                    log::info!("id={}, addr={}:{}", id, domain, port);

                    (id, format!("{}:{}", domain, port))
                }
                Action::Forward(id, Addr::Socket(addr)) if addr.port() == 0 => {
                    log::info!("id={}, addr={}", id, addr);

                    (id, self.config.forward_addr.clone())
                }
                Action::Forward(id, Addr::Socket(addr)) => {
                    log::info!("id={}, addr={}", id, addr);

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

        stream.send(Action::Connect(self.conv, id).into()).await?;

        Ok((stream, to))
    }
}

#[async_trait]
impl FusoListener<Reactor> for Fuso {
    #[inline]
    async fn accept(&mut self) -> Result<Reactor> {
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
    type Item = Reactor;

    #[inline]
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.accept_ax.poll_next(cx)
    }
}
