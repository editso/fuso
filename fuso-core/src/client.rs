use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};

use fuso_api::{
    async_trait, AsyncTcpSocketEx, Error, Forward, FusoListener, FusoPacket, Result, Spwan,
};

use futures::AsyncWriteExt;
use smol::{
    channel::{unbounded, Receiver},
    net::TcpStream,
};

use crate::retain::Heartbeat;
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
}

pub struct Fuso {
    accept_ax: Receiver<Reactor>,
}

pub struct Config {
    // 名称, 可选的
    pub name: Option<String>,
    // 服务端地址
    pub server_addr: SocketAddr,
    // 自定义服务绑定的端口, 如果不指定默认为随机分配
    pub server_bind_port: u16,
    // 桥接监听的地址
    pub bridge_addr: Option<SocketAddr>,
}

impl Fuso {
    pub async fn bind(config: Config) -> Result<Self> {
        let cfg = Arc::new(config);
        let server_addr = cfg.server_addr.clone();
        let server_bind_port = cfg.server_bind_port.clone();
        let bridge_addr = cfg.bridge_addr.clone();
        let name = cfg.name.clone();

        let mut stream = server_addr.tcp_connect().await?;

        stream
            .send(
                Action::Bind(name, {
                    if cfg.server_bind_port == 0 {
                        None
                    } else {
                        Some(SocketAddr::new(
                            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
                            server_bind_port,
                        ))
                    }
                })
                .into(),
            )
            .await?;

        let action: Action = stream.recv().await?.try_into()?;

        let (accept_tx, accept_ax) = unbounded();

        match action {
            Action::Accept(conv) => {
                log::debug!("Service binding is successful {}", conv);
                let mut stream = stream.guard(5000).await?;

                async move {
                    if bridge_addr.is_none() {
                        return;
                    }

                    let bridge_addr = bridge_addr.unwrap();

                    match Bridge::bind(bridge_addr, server_addr).await {
                        Ok(mut bridge) => {
                            log::info!("[bridge] Bridge service opened successfully");
                            loop {
                                match bridge.accept().await {
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
                        }
                        Err(e) => {
                            log::warn!("[bridge] Bridge service failed to open {}", e);
                        }
                    };
                }
                .detach();

                async move {
                    loop {
                        match stream.recv().await {
                            Err(e) => {
                                log::warn!("[fuc] Server disconnect {}", e);
                                break;
                            }
                            Ok(packet) => {
                                let action: Result<Action> = packet.try_into();
                                match action {
                                    Ok(Action::Ping) => {}
                                    Ok(action) => {
                                        match accept_tx
                                            .send(Reactor {
                                                conv,
                                                action,
                                                addr: server_addr,
                                            })
                                            .await
                                        {
                                            Err(_) => {
                                                let _ = stream.close().await;
                                                break;
                                            }
                                            _ => {}
                                        };
                                    }
                                    Err(e) => {
                                        log::debug!("{}", e);
                                    }
                                }
                            }
                        }
                    }
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
        let to = TcpStream::connect({
            match self.action {
                Action::Forward(Addr::Domain(domain, port)) => {
                    log::info!("connect {}:{}", domain, port);
                    format!("{}:{}", domain, port)
                }
                Action::Forward(Addr::Socket(addr)) => {
                    log::info!("connect {}", addr);
                    addr.to_string()
                }
                _ => {
                    return Err("Unsupported operation".into());
                }
            }
        })
        .await?;

        let mut stream = TcpStream::connect(self.addr)
            .await
            .map_err(|e| Error::with_io(e))?;

        stream.send(Action::Connect(self.conv).into()).await?;

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
