use std::{collections::VecDeque, net::SocketAddr, sync::Arc, time::Duration};

use bytes::Bytes;
use fuso_api::{async_trait, Error, ErrorKind, FusoPacket, Packet, Result};
use smol::{
    channel::{unbounded, Receiver, Sender},
    io::AsyncWriteExt,
    lock::Mutex,
    net::{TcpListener, TcpStream},
    Task,
};

use crate::{
    cmd::{CMD_CREATE, CMD_JOIN, CMD_PING},
    split, split_mutex,
};

struct Chief {
    io: TcpStream,
    task: Arc<Mutex<Vec<Task<()>>>>,
}

pub struct Fuso {
    accept_ax: Receiver<(TcpStream, TcpStream)>,
}

impl Fuso {
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
                        chief
                            .unwrap()
                            .join(
                                tcp.clone(),
                                {
                                    let (accept_ax, accept_tx) = unbounded();
                                    *mutex_sync.lock().await = Some(accept_ax.clone());
                                    accept_tx
                                },
                                accept_tx.clone(),
                            )
                            .await
                            .keep_live()
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

        let (accept_tx, accept_ax) = unbounded();
        let (accept_fuso_tx, accept_fuso_ax) = unbounded();

        smol::spawn(smol::future::race(
            Self::handler_at_client(at_tcp, accept_fuso_tx.clone()),
            Self::handler_fuso_at_client(at_fuso_tcp, (accept_tx.clone(), accept_fuso_ax)),
        ))
        .detach();

        Ok(Self { accept_ax })
    }
}

impl Chief {
    pub async fn auth(mut stream: TcpStream) -> Result<Chief> {
        let packet = stream.recv().await?;

        if packet.get_cmd() != CMD_JOIN {
            return Err(ErrorKind::BadPacket.into());
        }

        Ok(Self {
            io: stream,
            task: Arc::new(Mutex::new(Vec::new())),
        })
    }

    pub async fn keep_live(self) {
        let (mut reader, mut writer) = split(self.io.clone());

        smol::future::race(
            async move {
                loop {
                    match reader.recv().await {
                        Ok(_) => {}
                        Err(_) => {
                            log::warn!("[fus] Client disconnected");
                            break;
                        }
                    }
                }
            },
            async move {
                loop {
                    if let Err(_) = writer
                        .send(Packet::new(CMD_PING, Bytes::new()))
                        .await
                    {
                        log::warn!("[fus] The client does not respond for a long time, the system judges that the connection is invalid");
                        let _ = writer.close().await;
                        break;
                    };
                    smol::Timer::after(Duration::from_secs(5)).await;
                }
            },
        ).await;

        loop {
            if let Some(task) = self.task.lock().await.pop().take() {
                task.cancel().await;
            } else {
                break;
            }
        }
    }

    pub async fn join(
        self,
        tcp: TcpListener,
        accept_tx: Receiver<TcpStream>,
        accept_ax: Sender<(TcpStream, TcpStream)>,
    ) -> Self {
        log::debug!("[fus] Start processing {}", self.io.peer_addr().unwrap());

        let mut at_fuso = self.io.clone();
        let (produce_que, consume_que) = split_mutex(VecDeque::new());

        let future = smol::future::race(
            async move {
                let packet = Packet::new(CMD_CREATE, Bytes::new());
                loop {
                    if let Ok(mut stream) = accept_tx.recv().await {
                        if let Ok(()) = at_fuso.send(packet.clone()).await {
                            produce_que.lock().await.push_back(stream);
                        } else {
                            log::warn!("[fus] No response from client");

                            let _ = stream.close().await;
                            let _ = at_fuso.close().await;
                        };
                    }
                }
            },
            async move {
                loop {
                    let (mut stream, addr) = tcp
                        .accept()
                        .await
                        .map_err(|e| Error::with_io(e.into()))
                        .unwrap();

                    log::debug!("[fus] Client mapping connection received {}", addr);

                    let consume_que = consume_que.clone();
                    let accept_ax = accept_ax.clone();

                    smol::spawn(async move {
                        match stream.recv().await {
                            Ok(_) => {
                                if let Some(at_client) = consume_que.lock().await.pop_front() {
                                    if let Err(e) = accept_ax.send((at_client, stream)).await {
                                        log::warn!("[fus] Server exception {}", e);
                                    }
                                } else {
                                    log::warn!("[fus] No more connections");
                                    let _ = stream.close().await;
                                }
                            }
                            Err(e) => {
                                log::warn!("[fus] Client error {}", e);
                            }
                        }
                    })
                    .detach();
                }
            },
        );

        self.task.lock().await.push(smol::spawn(future));

        self
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
