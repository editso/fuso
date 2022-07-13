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
    ext::AsyncWriteExt, select::Select, time, Accepter, AsyncRead, AsyncWrite, Kind,
    UdpReceiverExt, UdpSocket,
};

use super::{third_party, KcpErr};

type BoxedFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

type Manager<C> = Arc<Mutex<HashMap<u64, HashMap<u32, Session<C>>>>>;
type Clean<T> = Arc<std::sync::Mutex<Vec<BoxedFuture<crate::Result<T>>>>>;
type CloseFn = Option<Box<dyn Fn() + Sync + Send + 'static>>;

pub fn now_mills() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u32
}

pub enum State<C> {
    Open((u64, u32, Session<C>)),
    Close(Session<C>),
}

#[derive(Debug, Clone, Default)]
pub struct Increment(Arc<Mutex<u32>>);

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
    close_waker: Option<Waker>,
}

pub struct KcpStream<C> {
    kcore: KcpCore<C>,
    close_fn: CloseFn,
    kcp_checker: BoxedFuture<crate::Result<()>>,
}

pub struct Session<C> {
    target: Option<SocketAddr>,
    kcore: KcpCore<C>,
}

pub struct KcpListener<C> {
    core: C,
    manager: Manager<C>,
    futures: Vec<BoxedFuture<crate::Result<State<C>>>>,
    close_futures: Clean<State<C>>,
}

pub struct KcpConnector<C> {
    core: C,
    increment: Increment,
    sessions: Arc<Mutex<HashMap<u32, Session<C>>>>,
    close_futures: Arc<std::sync::Mutex<Vec<BoxedFuture<()>>>>,
}

impl<C> Session<C>
where
    C: UdpSocket + Send + Unpin + 'static,
{
    pub async fn input(&mut self, data: Vec<u8>) -> crate::Result<()> {
        let mut kcp = self.kcore.kcp.lock().await;
        let mut next_input = &data[..];

        while next_input.len() > third_party::KCP_OVERHEAD {
            let n = kcp.input(next_input)?;
            next_input = &data[n..];
        }

        drop(kcp);

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
                let next = kcore.kcp.as_ref().lock().await.check(now_mills());

                log::trace!("next update {}ms", next);

                time::sleep(Duration::from_millis(next as u64)).await;

                let (wake_read, wake_write, is_dead) = {
                    let mut kcp = kcore.kcp.as_ref().lock().await;

                    kcp.update(now_mills()).await.unwrap();

                    let wake_read = kcp.peeksize().is_ok();

                    let wake_write = kcp.wait_snd() > 0;

                    (wake_read, wake_write, kcp.is_dead_link())
                };

                if wake_read {
                    kcore.try_wake_read();
                }

                if wake_write {
                    kcore.try_wake_write();
                }

                if is_dead {
                    break Err(KcpErr::IoError(std::io::ErrorKind::TimedOut.into()).into());
                }
            }
        })
    }

    pub fn new_server(core: KcpCore<C>, close_fn: CloseFn) -> Self {
        Self {
            kcore: core.clone(),
            kcp_checker: Self::async_checker(core),
            close_fn,
        }
    }

    pub fn new_client(
        kcp_guard: BoxedFuture<crate::Result<()>>,
        core: KcpCore<C>,
        close_fn: CloseFn,
    ) -> Self {
        let kcp_checker = Select::select(kcp_guard, Self::async_checker(core.clone()));
        Self {
            kcore: core,
            close_fn,
            kcp_checker: Box::pin(kcp_checker),
        }
    }
}

impl<C> KcpCore<C>
where C: UdpSocket + Send + Unpin  + 'static{
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

    async fn close(&self) -> crate::Result<()>{
        self.kcp.lock().await.close()
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
            close_waker: None,
        };

        Self { target, kcore }
    }

    pub async fn close(&self) -> crate::Result<()>{
        self.kcore.close().await
    }

    pub fn client_stream(
        &self,
        fut: BoxedFuture<crate::Result<()>>,
        close_fn: CloseFn,
    ) -> KcpStream<C> {
        KcpStream::new_client(fut, self.kcore.clone(), close_fn)
    }

    pub fn serve_stream(&self, close_fn: CloseFn) -> KcpStream<C> {
        KcpStream::new_server(self.kcore.clone(), close_fn)
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
            close_futures: Default::default(),
        })
    }

    async fn run_core(core: C, manager: Manager<C>) -> crate::Result<State<C>> {
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
                break Ok(State::Open((id, conv, session.clone())));
            }
        }
    }

    fn make_close_fn(&self, id: u64, conv: u32) -> CloseFn {
        let manager = self.manager.clone();
        let kcp_close = self.close_futures.clone();
        let do_close = {
            let manager = manager.clone();
            move || match kcp_close.lock() {
                Ok(mut kcp) => {
                    let manager = manager.clone();
                    kcp.push(Box::pin(async move {
                        if let Some(sessions) = manager.lock().await.get_mut(&id) {
                            log::debug!("close conv {}", conv);
                            match sessions.remove(&conv) {
                                Some(session) => Ok(State::Close(session)),
                                None => Err(KcpErr::Lock.into()),
                            }
                        } else {
                            Err(KcpErr::Lock.into())
                        }
                    }));
                }
                Err(e) => {
                    log::warn!("{}", e);
                }
            }
        };

        Some(Box::new(do_close))
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
        let futures = std::mem::replace(&mut self.futures, Default::default());

        let mut futures = futures.into_iter().chain(std::mem::replace(
            &mut *self.close_futures.lock()?,
            Default::default(),
        ));

        while let Some(mut future) = futures.next() {
            match Pin::new(&mut future).poll(cx) {
                Poll::Ready(Err(e)) => {
                    log::warn!("{}", e)
                }
                Poll::Ready(Ok(State::Close(kcp))) => {
                    log::debug!("close kcp");
                    drop(kcp)
                }
                Poll::Ready(Ok(State::Open((id, conv, kcp)))) => {
                    let accept_fut = Self::run_core(self.core.clone(), self.manager.clone());

                    self.futures.push(Box::pin(accept_fut));

                    self.futures.extend(futures);

                    let close_fn = self.make_close_fn(id, conv);

                    return Poll::Ready(Ok(kcp.serve_stream(close_fn)));
                }
                Poll::Pending => {
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
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<crate::Result<()>> {
        match self.kcp.try_lock_arc() {
            Some(mut kcp) => Poll::Ready(kcp.close()),
            None => {
                let waker = cx.waker().clone();
                drop(std::mem::replace(&mut self.close_waker, Some(waker)));
                Poll::Pending
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut std::task::Context<'_>) -> Poll<crate::Result<()>> {
        Poll::Ready(Ok(()))
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
        Poll::Ready(Ok(()))
    }
}

impl<C> Clone for KcpCore<C> {
    fn clone(&self) -> Self {
        KcpCore {
            kcp: self.kcp.clone(),
            write_waker: self.write_waker.clone(),
            read_waker: self.read_waker.clone(),
            close_waker: self.close_waker.clone(),
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
            increment: Default::default(),
            sessions: Default::default(),
            close_futures: Default::default(),
        }
    }

    fn safe_connect(&self, _: u32) -> BoxedFuture<crate::Result<()>> {
        let core = self.core.clone();
        let sessions = self.sessions.clone();
        let clean_futures = self.close_futures.clone();

        let fut = async move {
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

                let mut sessions = sessions.lock().await;
                if let Some(session) = sessions.get_mut(&conv) {
                    if let Err(e) = session.input(buf).await {
                        log::warn!("package error {}", e);
                        drop(session);
                        let mut session = unsafe { sessions.remove(&conv).unwrap_unchecked() };
                        let _ = session.kcore.flush().await;
                        let _ = session.kcore.close().await;
                    }
                }

                drop(sessions);

                let mut futures = match clean_futures.lock() {
                    Err(e) => return Err(e.into()),
                    Ok(mut futures) => std::mem::replace(&mut *futures, Default::default()),
                };

                while let Some(futures) = futures.pop() {
                    futures.await;
                }
            }
        };

        Box::pin(fut)
    }

    pub async fn connect(&self) -> crate::Result<KcpStream<C>> {
        let conv = {
            let sessions = self.sessions.lock().await;
            let conv = self.increment.current().await;
            let mut next_conv = conv;
            loop {
                if !sessions.contains_key(&next_conv) {
                    break next_conv;
                }

                if next_conv == conv {
                    return Err(KcpErr::NoMoreConv.into());
                }

                next_conv = self.increment.next().await;
            }
        };

        let (kcp_close, sessions) = (self.close_futures.clone(), self.sessions.clone());

        let session = Session::new(conv, None, self.core.clone());

        self.sessions.lock().await.insert(conv, session.clone());

        let fut = self.safe_connect(conv);

        let close_fn = move || match kcp_close.lock() {
            Ok(mut close) => {
                let sessions = sessions.clone();
                close.push(Box::pin(async move {
                    if let Some(session) = sessions.lock().await.remove(&conv) {
                        drop(session.close().await)
                    }
                }))
            }
            Err(e) => {
                log::warn!("{}", e)
            }
        };

        Ok(session.client_stream(Box::pin(fut), Some(Box::new(close_fn))))
    }
}

impl Increment {
    pub async fn current(&self) -> u32 {
        *self.0.lock().await
    }

    pub async fn next(&self) -> u32 {
        let mut inc = self.0.lock().await;
        let (next, overflow) = inc.overflowing_add(1);
        *inc = if overflow { 0 } else { next };
        next
    }
}


impl<C> Drop for KcpStream<C> {
    fn drop(&mut self) {
        if let Some(close_fn) = self.close_fn.take() {
            log::debug!("close kcp");
            close_fn()
        }
    }
}
