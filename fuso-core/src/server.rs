use std::{collections::VecDeque, net::SocketAddr, sync::Arc};

use bytes::Bytes;
use fuso_api::{async_trait, Error, ErrorKind, FusoPacket, Packet, Result, Spwan};

use smol::{
    channel::{unbounded, Receiver, Sender},
    future::FutureExt,
    io::AsyncWriteExt,
    lock::Mutex,
    net::{TcpListener, TcpStream},
};

use crate::{
    cmd::{CMD_CREATE, CMD_JOIN},
    retain::{HeartGuard, Heartbeat},
    split_mutex,
};

struct Chief<IO> {
    io: IO,
}

pub struct Fuso {
    visit_addr: SocketAddr,
    bind_addr: SocketAddr,
    accept_ax: Receiver<(TcpStream, TcpStream)>,
}

impl Fuso {
    #[inline]
    async fn handler_at_client(tcp: TcpListener, accept_tx: Sender<TcpStream>) -> Result<()> {
        log::info!("[fus] Actual access address {}", tcp.local_addr().unwrap());

        loop {
            let stream = tcp.accept().await;

            if stream.is_err() {
                break Err(stream.unwrap_err().into());
            }

            let (stream, addr) = stream.unwrap();

            log::debug!("[fus] Visitor connections from {}", addr);

            if let Err(e) = accept_tx.send(stream).await {
                break Err(Error::with_io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )));
            };
        }
    }

    #[inline]
    async fn handler_fuso_at_client(
        tcp: TcpListener,
        (accept_tx, accept_ax): (Sender<(TcpStream, TcpStream)>, Receiver<TcpStream>),
    ) -> Result<()> {
        log::info!("[fus] Server started! {}", tcp.local_addr().unwrap());
        log::info!("[fus] Waiting for client connection");

        let mutex_sync: Arc<Mutex<Option<Sender<TcpStream>>>> = Arc::new(Mutex::new(None));

        smol::future::race(
            {
                let mutex_sync = mutex_sync.clone();
                async move {
                    loop {
                        let stream = accept_ax.recv().await;

                        if stream.is_err() {
                            break Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                stream.unwrap_err().to_string(),
                            )
                            .into());
                        }

                        let mut stream = stream.unwrap();

                        let mut mutex_sync = mutex_sync.lock().await;
                        if let Some(accept_ax) = mutex_sync.as_mut() {
                            if let Err(_) = accept_ax.send(stream.clone()).await {
                                accept_ax.close();
                                let _ = mutex_sync.take();
                                let _ = stream.close().await;
                            }
                        } else {
                            log::debug!("[fus] Client is not ready");
                            let _ = stream.close().await;
                        }
                    }
                }
            },
            async move {
                loop {
                    let stream = tcp.accept().await;

                    if stream.is_err() {
                        break Err(stream.unwrap_err().into());
                    }

                    let (stream, addr) = stream.unwrap();

                    log::debug!("[fus] Client connection {}", addr);

                    let chief = Chief::auth(stream).await;

                    if chief.is_err() {
                        log::warn!("[fus] Client authentication failed");
                    } else {
                        let (accept_ax, accept_fuso_tx) = unbounded();

                        *mutex_sync.lock().await = Some(accept_ax.clone());

                        chief
                            .unwrap()
                            .join(tcp.clone(), accept_fuso_tx, accept_tx.clone())
                            .await;
                    }
                }
            },
        )
        .await
    }

    pub async fn bind(
        (bind_at_client_addr, bind_at_fuso_client_addr): (SocketAddr, SocketAddr),
    ) -> Result<Self> {
        let (at_tcp, at_fuso_tcp) = (
            TcpListener::bind(bind_at_client_addr)
                .await
                .map_err(|e| Error::with_io(e))?,
            TcpListener::bind(bind_at_fuso_client_addr)
                .await
                .map_err(|e| Error::with_io(e))?,
        );

        let visit_addr = at_tcp.local_addr().unwrap();
        let bind_addr = at_fuso_tcp.local_addr().unwrap();

        let (accept_tx, accept_ax) = unbounded();
        let (accept_fuso_tx, accept_fuso_ax) = unbounded();

        smol::spawn(smol::future::race(
            Self::handler_at_client(at_tcp, accept_fuso_tx.clone()),
            Self::handler_fuso_at_client(at_fuso_tcp, (accept_tx.clone(), accept_fuso_ax)),
        ))
        .detach();

        Ok(Self {
            accept_ax,
            visit_addr,
            bind_addr,
        })
    }

    pub fn visit_addr(&self) -> SocketAddr {
        self.visit_addr
    }

    pub fn bind_addr(&self) -> SocketAddr {
        self.bind_addr
    }
}

impl Chief<HeartGuard<TcpStream>> {
    #[inline]
    pub async fn auth(mut stream: TcpStream) -> Result<Chief<HeartGuard<TcpStream>>> {
        let packet = stream.recv().await?;

        if packet.get_cmd() != CMD_JOIN {
            return Err(ErrorKind::BadPacket.into());
        }

        let guard = stream.guard(5000).await?;

        Ok(Self { io: guard })
    }

    pub async fn join(
        self,
        tcp: TcpListener,
        accept_tx: Receiver<TcpStream>,
        accept_ax: Sender<(TcpStream, TcpStream)>,
    ) {
        let (produce_que, consume_que) = split_mutex(VecDeque::new());

        let client_future = {
            let mut io = self.io.clone();
            async move {
                loop {
                    match io.recv().await {
                        Ok(_) => {
                            // log::debug!("recv {:?}", packet);
                        }
                        Err(_) => {
                            log::warn!("[fus] Client disconnect");
                            break;
                        }
                    }
                }
            }
        };

        let visit_future = {
            let mut io = self.io.clone();
            async move {
                let packet = Packet::new(CMD_CREATE, Bytes::new());
                loop {
                    if let Ok(mut stream) = accept_tx.recv().await {
                        if let Ok(()) = io.send(packet.clone()).await {
                            produce_que.lock().await.push_back(stream);
                        } else {
                            log::warn!("[fus] No response from client");

                            let _ = stream.close().await;
                            let _ = io.close().await;
                        };
                    }
                }
            }
        };

        let fuso_future = async move {
            loop {
                let stream = tcp.accept().await;

                if stream.is_err() {
                    log::debug!("[fus] Server error {}", stream.unwrap_err());
                    break;
                }

                let (mut stream, addr) = stream.unwrap();

                log::debug!("[fus] Client mapping connection received {}", addr);

                let consume_que = consume_que.clone();
                let accept_ax = accept_ax.clone();

                let future = async move {
                    let packet = stream.recv().await;

                    if packet.is_err() {
                        log::warn!("[fus] Client error {}", packet.unwrap_err());
                    } else {
                        let from = consume_que.lock().await.pop_front();

                        if from.is_none() {
                            let _ = stream.close().await;
                        } else {
                            if let Err(e) = accept_ax.send((from.unwrap(), stream.clone())).await {
                                log::warn!("[fus] Server exception {}", e);
                                let _ = stream.close().await;
                            }
                        }
                    }
                };

                future.detach();
            }
        };

        client_future.race(visit_future.race(fuso_future)).await;
    }
}

#[async_trait]
impl fuso_api::FusoListener<(TcpStream, TcpStream)> for Fuso {
    #[inline]
    async fn accept(&mut self) -> Result<(TcpStream, TcpStream)> {
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
