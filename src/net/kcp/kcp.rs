use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    future::Future,
    hash::{Hash, Hasher},
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Poll, Waker},
    time::Duration,
};

use async_mutex::Mutex;

use crate::{
    select::Select, time, Accepter, AsyncRead, AsyncWrite, Kind, UdpReceiverExt, UdpSocket,
};

use super::{third_party, KcpErr};

type BoxedFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

type Manager<C> = Arc<Mutex<HashMap<u64, HashMap<u32, Session<C>>>>>;

pub fn now_mills() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u32
}

pub enum AsyncState<C> {
    Create(Session<C>),
}

#[pin_project::pin_project]
pub struct KOutput<C> {
    #[pin]
    output: C,
    target: Option<SocketAddr>,
}

pub struct KcpCore<C> {
    kcp: Arc<Mutex<third_party::Kcp<KOutput<C>>>>,
    read_waker: Option<Waker>,
    write_waker: Option<Waker>,
}

pub struct KcpStream<C> {
    kcore: KcpCore<C>,
    kcp_checker: BoxedFuture<crate::Result<()>>,
}

pub struct Session<C> {
    target: Option<SocketAddr>,
    kcore: KcpCore<C>,
}

pub struct KcpListener<C> {
    core: C,
    manager: Manager<C>,
    futures: Vec<BoxedFuture<crate::Result<AsyncState<C>>>>,
}

pub struct KcpConnector<C> {
    core: C,
    id_conv: u32,
    sessions: Arc<Mutex<HashMap<u32, Session<C>>>>,
}

impl<C> Session<C>
where
    C: UdpSocket + Unpin + 'static,
{
    pub async fn input(&mut self, data: Vec<u8>) -> crate::Result<()> {
        {
            let mut kcp = self.kcore.kcp.lock().await;
            let n = kcp.input(&data)?;
        }

        self.kcore.try_wake_read();

        Ok(())
    }
}

impl<C> KcpStream<C>
where
    C: UdpSocket + Unpin + Send + Sync + 'static,
{
    fn async_checker(mut kcore: KcpCore<C>) -> BoxedFuture<crate::Result<()>> {
        Box::pin(async move {
            loop {
                // log::debug!("kcp checker");

                let next = kcore.kcp.as_ref().lock().await.check(now_mills());

                time::sleep(Duration::from_millis(next as u64)).await;

                let (wake_read, wake_write) = {
                    let mut kcp = kcore.kcp.as_ref().lock().await;

                    kcp.update(now_mills()).await.unwrap();

                    let wake_read = kcp.peeksize().is_ok();

                    let wake_write = kcp.wait_snd() > 0;

                    (wake_read, wake_write)
                };

                if wake_read {
                    kcore.try_wake_read();
                }

                if wake_write {
                    kcore.try_wake_write();
                }
            }
        })
    }

    pub fn new_server(core: KcpCore<C>) -> Self {
        Self {
            kcore: core.clone(),
            kcp_checker: Self::async_checker(core),
        }
    }

    pub fn new_client(kcp_guard: BoxedFuture<crate::Result<()>>, core: KcpCore<C>) -> Self {
        let kcp_checker = Select::select(kcp_guard, Self::async_checker(core.clone()));

        Self {
            kcore: core,
            kcp_checker: Box::pin(kcp_checker),
        }
    }
}

impl<C> KcpCore<C> {
    fn try_wake_read(&mut self) {
        if let Some(waker) = self.read_waker.take() {
            waker.wake();
        }
    }

    fn try_wake_write(&mut self) {
        if let Some(waker) = self.write_waker.take() {
            waker.wake()
        }
    }
}

impl<C> Session<C>
where
    C: UdpSocket + Unpin + Send + Sync + 'static,
{
    pub fn new(conv: u32, target: Option<SocketAddr>, output: C) -> Self {
        let output = KOutput {
            output,
            target: target.clone(),
        };

        let kcore = KcpCore {
            kcp: { Arc::new(Mutex::new(third_party::Kcp::new(conv, output))) },
            write_waker: None,
            read_waker: None,
        };

        Self { target, kcore }
    }

    pub fn client(&self, fut: BoxedFuture<crate::Result<()>>) -> KcpStream<C> {
        KcpStream::new_client(fut, self.kcore.clone())
    }

    pub fn stream(&self) -> KcpStream<C> {
        KcpStream::new_server(self.kcore.clone())
    }
}

impl<C> KcpListener<C>
where
    C: UdpSocket + Send + Sync + Clone + Unpin + 'static,
{
    pub fn bind(core: C) -> crate::Result<Self> {
        let manager: Manager<C> = Default::default();

        let core_fut = Box::pin(Self::run_core(core.clone(), manager.clone()));

        Ok(Self {
            core,
            manager,
            futures: vec![core_fut],
        })
    }

    async fn run_core(core: C, manager: Manager<C>) -> crate::Result<AsyncState<C>> {
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
                .or_insert_with(|| Session::new(conv, Some(addr), core.clone()));

            if let Err(e) = session.input(data).await {
                log::warn!("{}", e);
                manager.remove(&conv);
            } else if new_kcp {
                break Ok(AsyncState::Create(session.clone()));
            }
        }
    }
}

impl<C> Accepter for KcpListener<C>
where
    C: UdpSocket + Send + Clone + Unpin + Sync + 'static,
{
    type Stream = KcpStream<C>;

    fn poll_accept(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<Self::Stream>> {
        let mut futures = std::mem::replace(&mut self.futures, Default::default());

        while let Some(mut future) = futures.pop() {
            match Pin::new(&mut future).poll(cx) {
                std::task::Poll::Ready(Err(e)) => {
                    log::warn!("{}", e)
                }
                std::task::Poll::Ready(Ok(AsyncState::Create(session))) => {
                    let accept_fut = Self::run_core(self.core.clone(), self.manager.clone());

                    self.futures.push(Box::pin(accept_fut));

                    self.futures.extend(futures);

                    return Poll::Ready(Ok(session.stream()));
                }
                std::task::Poll::Pending => {
                    self.futures.push(future);
                }
            }
        }

        Poll::Pending
    }
}

impl<C> AsyncWrite for KcpCore<C>
where
    C: UdpSocket + Unpin + 'static,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<crate::Result<usize>> {
        match self.kcp.try_lock_arc() {
            Some(mut kcp) => Poll::Ready(kcp.send(buf)),
            None => {
                let waker = cx.waker().clone();
                drop(std::mem::replace(&mut self.write_waker, Some(waker)));
                Poll::Pending
            }
        }
    }

    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<crate::Result<()>> {
        unimplemented!()
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<crate::Result<()>> {
        unimplemented!()
    }
}

impl<C> AsyncRead for KcpCore<C>
where
    C: UdpSocket + Unpin + 'static,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> Poll<crate::Result<usize>> {
        match self.kcp.try_lock_arc() {
            None => {
                let waker = cx.waker().clone();
                drop(std::mem::replace(&mut self.read_waker, Some(waker)));
                Poll::Pending
            }
            Some(mut kcp) => match kcp.recv(buf.initialize_unfilled()) {
                Ok(n) => {
                    buf.advance(n);

                    return Poll::Ready(Ok(n));
                }
                Err(e) => match e.kind() {
                    Kind::Kcp(KcpErr::RecvQueueEmpty) => {
                        let waker = cx.waker().clone();
                        drop(std::mem::replace(&mut self.read_waker, Some(waker)));
                        return Poll::Pending;
                    }
                    _ => unimplemented!(),
                },
            },
        }
    }
}

impl<C> AsyncRead for KcpStream<C>
where
    C: UdpSocket + Unpin + 'static,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> std::task::Poll<crate::Result<usize>> {
        match Pin::new(&mut self.kcp_checker).poll(cx)? {
            Poll::Ready(()) => return Poll::Ready(Ok(0)),
            Poll::Pending => Pin::new(&mut self.kcore).poll_read(cx, buf),
        }
    }
}

impl<C> AsyncWrite for KcpStream<C>
where
    C: UdpSocket + Unpin + 'static,
{
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::Result<usize>> {
        match Pin::new(&mut self.kcp_checker).poll(cx)? {
            Poll::Ready(()) => return Poll::Ready(Ok(0)),
            Poll::Pending => Pin::new(&mut self.kcore).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        match Pin::new(&mut self.kcp_checker).poll(cx)? {
            Poll::Ready(()) => return Poll::Ready(Ok(())),
            Poll::Pending => Pin::new(&mut self.kcore).poll_flush(cx),
        }
    }

    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        match Pin::new(&mut self.kcp_checker).poll(cx)? {
            Poll::Ready(()) => return Poll::Ready(Ok(())),
            Poll::Pending => Pin::new(&mut self.kcore).poll_close(cx),
        }
    }
}

impl<C> AsyncWrite for KOutput<C>
where
    C: UdpSocket + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        data: &[u8],
    ) -> Poll<crate::Result<usize>> {
        let this = self.project();
        let output = Pin::new(&*this.output);
        match this.target.as_ref() {
            None => output.poll_send(cx, data),
            Some(addr) => output.poll_send_to(cx, addr, data),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut std::task::Context<'_>) -> Poll<crate::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut std::task::Context<'_>) -> Poll<crate::Result<()>> {
        unimplemented!()
    }
}

impl<C> Clone for KcpCore<C> {
    fn clone(&self) -> Self {
        KcpCore {
            kcp: self.kcp.clone(),
            write_waker: self.write_waker.clone(),
            read_waker: self.read_waker.clone(),
        }
    }
}

impl<C> Clone for Session<C> {
    fn clone(&self) -> Self {
        Self {
            kcore: self.kcore.clone(),
            target: self.target.clone(),
        }
    }
}

impl<C> KcpConnector<C>
where
    C: UdpSocket + Unpin + Clone + Send + Sync + 'static,
{
    pub fn new(core: C) -> Self {
        Self {
            core: core,
            id_conv: 0,
            sessions: Default::default(),
        }
    }

    pub async fn connect(&self) -> KcpStream<C> {
        let session = Session::new(1, None, self.core.clone());

        self.sessions.lock().await.insert(1, session.clone());

        let fut = {
            let mut core = self.core.clone();
            let sessions = self.sessions.clone();
            async move {
                loop {
                    let mut buf = Vec::with_capacity(1500);

                    unsafe {
                        buf.set_len(1500);
                    }

                    let n = core.recv(&mut buf).await?;
                    buf.truncate(n);

                    if n < third_party::KCP_OVERHEAD {
                        log::warn!("bad kcp packet");
                        continue;
                    }

                    let conv = third_party::get_conv(&buf);

                    match sessions.lock().await.get_mut(&conv) {
                        None => {
                            log::warn!("bad kcp");
                        }
                        Some(session) => {
                            if let Err(e) = session.input(buf).await {
                                log::warn!("error {}", e);
                            }
                        }
                    }
                }
            }
        };

        session.client(Box::pin(fut))
    }
}
