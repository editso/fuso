use std::{net::SocketAddr, sync::Arc};

use futures::TryStreamExt;
use smol::{
    channel::{unbounded, Receiver},
    net::TcpStream,
    Task,
};

use async_trait::async_trait;
use fuso_api::{AsyncTcpSocketEx, Error, FusoListener, Result, Spwan};

pub struct Bridge {
    accept_ax: Receiver<(TcpStream, TcpStream)>,
    task: Arc<std::sync::Mutex<Option<Task<()>>>>,
}

impl Bridge {
    #[inline]
    pub async fn bind(bind_addr: SocketAddr, server_addr: SocketAddr) -> Result<Self> {
        let listen = bind_addr.tcp_listen().await?;

        let (accept_tx, accept_ax) = unbounded();

        let task = async move {
            log::info!(
                "[bridge] Bridge service started {}",
                listen.local_addr().unwrap()
            );

            let _ = listen
                .incoming()
                .try_fold(accept_tx, |accept_tx, from| async move {
                    {
                        let accept_tx = accept_tx.clone();
                        if accept_tx.is_closed() {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "receiver closed",
                            ));
                        }

                        async move {
                            match server_addr.tcp_connect().await {
                                Ok(to) => {
                                    let _ = accept_tx.send((from, to)).await;
                                }
                                Err(e) => {
                                    log::error!(
                                        "[bridge] connect to server fialure {} {}",
                                        e,
                                        server_addr
                                    );
                                }
                            }
                        }
                    }
                    .detach();

                    Ok(accept_tx)
                })
                .await;
        };

        Ok(Bridge {
            accept_ax,
            task: Arc::new(std::sync::Mutex::new(Some(smol::spawn(task)))),
        })
    }
}

impl Drop for Bridge {
    #[inline]
    fn drop(&mut self) {
        if let Some(task) = self.task.lock().unwrap().take() {
            async move {
                task.cancel().await;
                log::warn!("[bridge] Bridge service stopped");
            }
            .detach();
        }
    }
}

#[async_trait]
impl FusoListener<(TcpStream, TcpStream)> for Bridge {
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
        let _ = self.accept_ax.clone();
        Ok(())
    }
}
