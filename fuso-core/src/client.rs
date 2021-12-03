use std::net::SocketAddr;

use fuso_api::{async_trait, Error, Forward, FusoListener, FusoPacket, Result, Spwan};

use futures::AsyncWriteExt;
use smol::{
    channel::{unbounded, Receiver},
    net::TcpStream,
};

use crate::retain::Heartbeat;
use crate::{bridge::Bridge, packet::Action};

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

impl Fuso {
    pub async fn bind(addr: SocketAddr, bind_port: u16, bridge_bind: u16) -> Result<Self> {
        // let stream = addr.tcp_connect().await?;

        let mut stream = TcpStream::connect(addr)
            .await
            .map_err(|e| Error::with_io(e))?;

        stream
            .send(
                Action::Bind({
                    if bind_port == 0 {
                        None
                    } else {
                        Some(format!("0.0.0.0:{}", bind_port).parse().unwrap())
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
                    if bridge_bind == 0 {
                        return;
                    }

                    match Bridge::bind(format!("0.0.0.0:{}", bridge_bind).parse().unwrap(), addr)
                        .await
                    {
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
                                        match accept_tx.send(Reactor { conv, action, addr }).await {
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
    pub async fn join(self) -> Result<TcpStream> {
        let mut stream = TcpStream::connect(self.addr)
            .await
            .map_err(|e| Error::with_io(e))?;
        stream.send(Action::Connect(self.conv).into()).await?;
        Ok(stream)
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
