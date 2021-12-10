use async_trait::async_trait;
use fuso_api::{now_mills, FusoPacket, Spawn};
use futures::{AsyncRead, AsyncWrite, AsyncWriteExt};
use smol::{net::TcpStream, Task};
use std::{io::Result, net::SocketAddr};
use std::{
    ops::Sub,
    pin::Pin,
    sync::{Arc, RwLock},
    task::Poll,
    time::Duration,
};

use crate::packet::Action;

#[derive(Clone)]
pub struct HeartGuard<T> {
    target: T,
    last: Arc<RwLock<u64>>,
    guard: Arc<std::sync::Mutex<Option<Task<()>>>>,
}

#[async_trait]
pub trait Heartbeat<T> {
    async fn guard(self, interval: u64) -> Result<HeartGuard<T>>;
}

impl<T> HeartGuard<T>
where
    T: Clone + AsyncRead + AsyncWrite + Unpin + Send + Sync + 'static,
{
    pub fn new(target: T, interval: u64) -> Self {
        let last = Arc::new(RwLock::new(now_mills()));

        Self {
            last: last.clone(),
            target: target.clone(),
            guard: Arc::new(std::sync::Mutex::new(Some(smol::spawn({
                let mut io = target.clone();

                async move {
                    log::info!("Guardian mode is turned on");

                    loop {
                        let time = now_mills();
                        let last = *last.read().unwrap();

                        if now_mills().sub(last).ge(&interval) {
                            log::debug!("Client is dead");
                            let _ = io.close().await;
                            break;
                        } else {
                            let interval = interval - (time - last);

                            log::debug!("Next check interval={}mss", interval);

                            smol::Timer::after(Duration::from_millis(interval)).await;

                            if let Err(_) = io.send(Action::Ping.into()).await {
                                let _ = io.close().await;
                                break;
                            }
                        }
                    }
                }
            })))),
        }
    }
}

impl<T> Drop for HeartGuard<T> {
    #[inline]
    fn drop(&mut self) {
        if let Some(guard) = self.guard.lock().unwrap().take() {
            async move {
                guard.cancel().await;
                log::debug!("Guard is off");
            }
            .detach();
        }
    }
}

#[async_trait]
impl<T> Heartbeat<T> for T
where
    T: Clone + AsyncRead + AsyncWrite + Unpin + Send + Sync + 'static,
{
    #[inline]
    async fn guard(self, interval: u64) -> Result<HeartGuard<T>> {
        Ok(HeartGuard::new(self, interval))
    }
}

impl<T> AsyncRead for HeartGuard<T>
where
    T: Clone + AsyncRead + Unpin + Send + Sync + 'static,
{
    #[inline]
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match Pin::new(&mut self.target).poll_read(cx, buf) {
            std::task::Poll::Ready(result) => {
                if result.is_ok() {
                    *self.last.write().unwrap() = now_mills();
                }

                Poll::Ready(result)
            }
            std::task::Poll::Pending => Poll::Pending,
        }
    }
}

impl<T> AsyncWrite for HeartGuard<T>
where
    T: Clone + AsyncWrite + Unpin + Send + Sync + 'static,
{
    #[inline]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        Pin::new(&mut self.target).poll_write(cx, buf)
    }

    #[inline]
    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.target).poll_flush(cx)
    }

    #[inline]
    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.target).poll_close(cx)
    }
}

impl HeartGuard<TcpStream> {
    #[inline]
    pub fn lock_addr(&self) -> std::io::Result<SocketAddr> {
        self.target.local_addr()
    }

    #[inline]
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.target.peer_addr()
    }
}
