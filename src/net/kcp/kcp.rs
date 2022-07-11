use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    future::Future,
    hash::{Hash, Hasher},
    marker::PhantomData,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::Poll,
};

use async_mutex::Mutex;

use crate::{
    ready, Accepter, Addr, AsyncRead, AsyncWrite, FactoryWrapper, UdpReceiverExt, UdpSocket,
};

use super::third_party;

type BoxedFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

type Manager<C> = Arc<Mutex<HashMap<u64, HashMap<u32, Session<C>>>>>;

pub struct KOutput<C> {
    output: C,
    target: Option<SocketAddr>,
}

pub struct KcpCore<C> {
    kcore: Arc<Mutex<third_party::Kcp<KOutput<C>>>>,
}

pub struct KcpStream<C>(KcpCore<C>);

pub struct Session<C> {
    kcore: KcpCore<C>,
}

pub struct KcpListener<C> {
    core: C,
    manager: Manager<C>,
    futures: Vec<BoxedFuture<crate::Result<KcpStream<C>>>>,
}

impl<C> Session<C> {
    pub async fn input(&mut self, data: Vec<u8>) -> crate::Result<()> {
        unimplemented!()
    }
}

impl<C> KcpStream<C> {
    pub fn new(core: KcpCore<C>) -> Self {
        Self(core)
    }
}

impl<C> Session<C>
where
    C: UdpSocket + Unpin + 'static,
{
    pub fn new(conv: u32, target: SocketAddr, output: C) -> Self {
        let output = KOutput {
            output,
            target: Some(target),
        };

        Self {
            kcore: KcpCore {
                kcore: { Arc::new(Mutex::new(third_party::Kcp::new(conv, output))) },
            },
        }
    }

    pub fn stream(&self) -> KcpStream<C> {
        unimplemented!()
    }
}

impl<C> KcpListener<C>
where
    C: UdpSocket + Send + Clone + Unpin + 'static,
{
    pub fn bind<U>(core: C) -> crate::Result<Self> {
        let manager: Manager<C> = Default::default();

        let core_fut = Box::pin(Self::run_core(core.clone(), manager.clone()));

        Ok(Self {
            core,
            manager,
            futures: vec![core_fut],
        })
    }

    async fn run_core(mut core: C, manager: Manager<C>) -> crate::Result<KcpStream<C>> {
        loop {
            let mut data = Vec::with_capacity(1500);

            unsafe {
                data.set_len(1500);
            }

            let (n, addr) = core.recv_from(&mut data).await?;
            data.truncate(n);

            let id = {
                let mut hasher = DefaultHasher::new();
                addr.hash(&mut hasher);
                hasher.finish()
            };

            if n < third_party::KCP_OVERHEAD {
                log::warn!("received an invalid kcp packet");
                continue;
            }

            let conv = third_party::get_conv(&data);

            let mut manager = manager.lock().await;
            let manager = manager.entry(id).or_default();

            let new_kcp = !manager.contains_key(&conv);

            let session = manager
                .entry(conv)
                .or_insert_with(|| Session::new(conv, addr, core.clone()));

            if let Err(e) = session.input(data).await {
                log::warn!("{}", e);
                manager.remove(&conv);
            } else if new_kcp {
                break Ok(session.stream());
            }
        }
    }
}

impl<C> Accepter for KcpListener<C>
where
    C: Unpin,
{
    type Stream = KcpStream<C>;

    fn poll_accept(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<Self::Stream>> {
        let mut futures = std::mem::replace(&mut self.futures, Default::default());

        while let Some(mut future) = futures.pop() {
            match Pin::new(&mut future).poll(cx) {
                std::task::Poll::Ready(kcp) => return Poll::Ready(kcp),
                std::task::Poll::Pending => {
                    self.futures.push(future);
                }
            }
        }

        Poll::Pending
    }
}

impl<C> AsyncRead for KcpStream<C> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> std::task::Poll<crate::Result<usize>> {
        unimplemented!()
    }
}

impl<C> AsyncWrite for KcpStream<C> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::Result<usize>> {
        todo!()
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        todo!()
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        todo!()
    }
}

impl<C> third_party::KcpOutput for KOutput<C>
where
    C: UdpSocket + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        data: &[u8],
    ) -> Poll<crate::Result<usize>> {
        let output = Pin::new(&mut self.output);
        match self.target.as_ref() {
            None => output.poll_send(cx, data),
            Some(addr) => output.poll_send_to(cx, addr, data),
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<crate::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl<C> Clone for KcpCore<C> {
    fn clone(&self) -> Self {
        KcpCore {
            kcore: self.kcore.clone(),
        }
    }
}

impl<C> Clone for Session<C> {
    fn clone(&self) -> Self {
        Self {
            kcore: self.kcore.clone(),
        }
    }
}
