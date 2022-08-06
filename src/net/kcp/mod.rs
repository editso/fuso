mod third_party;

pub use third_party::KcpErr;

mod stream;
pub use stream::*;

mod builder;
pub use builder::*;

use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    future::Future,
    hash::{Hash, Hasher},
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Poll, Waker},
    time::{Duration, Instant},
};

use async_mutex::Mutex;

use crate::{
    guard::buffer::Buffer, time, Accepter, Address, Executor, NetSocket, Socket, SocketKind, Task,
    UdpReceiverExt, UdpSocket,
};

type BoxedFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

type Manager<C> = Arc<Mutex<HashMap<u64, HashMap<u32, KLife<C>>>>>;
type Callback = Option<Box<dyn FnOnce() + Sync + Send + 'static>>;

pub fn now_mills() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u32
}

pub enum State<C> {
    Open((u64, u32, Session<C>, Address)),
    Close(u32),
}

enum UpdateState {
    Updating(Pin<Box<dyn Future<Output = crate::Result<()>> + 'static>>),
    WaitUpdate(Pin<Box<dyn Future<Output = crate::Result<()>> + 'static>>),
}

pub enum KLife<C> {
    Dead(Instant, Session<C>),
    Active(Session<C>),
}

pub struct KcpUpdate<C> {
    kcore: Arc<std::sync::Mutex<KcpCore<C>>>,
    kupdate: Option<UpdateState>,
}

#[derive(Debug, Clone)]
pub struct Increment(Arc<Mutex<u32>>);

pub struct Session<C> {
    pub(crate) conv: u32,
    pub(crate) target: Option<SocketAddr>,
    pub(crate) kcore: Arc<std::sync::Mutex<KcpCore<C>>>,
}

pub struct KcpListener<C, E> {
    pub(crate) core: C,
    pub(crate) manager: Manager<C>,
    pub(crate) futures: Vec<BoxedFuture<crate::Result<State<C>>>>,
    pub(crate) executor: E,
}

pub struct KcpConnector<C, E> {
    core: C,
    task: Task<crate::Result<()>>,
    executor: E,
    sessions: Arc<Mutex<HashMap<u32, KLife<C>>>>,
    increment: Increment,
}

impl<C> Session<C>
where
    C: UdpSocket + Send + Unpin + 'static,
{
    pub async fn input(&mut self, data: Vec<u8>) -> crate::Result<()> {
        let mut kcore = self.kcore.lock()?;
        let mut next_input = &data[..];

        while next_input.len() >= third_party::KCP_OVERHEAD {
            let n = kcore.input(next_input)?;
            next_input = &data[n..];
        }

        if kcore.peeksize().is_ok() {
            kcore.try_wake_read();
        }

        if kcore.wait_snd() > 0 {
            kcore.try_wake_write();
        }

        Ok(())
    }
}

impl<C> KcpStream<C>
where
    C: UdpSocket + Unpin + Send + Sync + 'static,
{
    pub fn new(
        conv: u32,
        local_addr: Address,
        peer_addr: Address,
        kcore: Arc<std::sync::Mutex<KcpCore<C>>>,
        close_callback: Callback,
    ) -> Self {
        Self {
            conv,
            kcore,
            local_addr: local_addr.with_kind(SocketKind::Kcp),
            peer_addr: peer_addr.with_kind(SocketKind::Kcp),
            close_callback,
        }
    }
}

impl<C> KcpCore<C>
where
    C: UdpSocket + Send + Unpin + 'static,
{
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
    fn new<E, F, Fut>(
        conv: u32,
        target: Option<SocketAddr>,
        output: C,
        executor: E,
        clean_callback: F,
    ) -> crate::Result<Self>
    where
        E: Executor + Send + 'static,
        F: FnOnce(u32) -> Fut + Sync + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let output = KOutput {
            output,
            target: target.clone(),
        };

        let mut kcp = third_party::Kcp::new(conv, output);

        kcp.set_wndsize(1024, 1024);
        kcp.set_nodelay(true, 20, 2, true);
        kcp.set_maximum_resend_times(10);

        let kcore = Arc::new(std::sync::Mutex::new(KcpCore {
            kcp,
            kbuf: Buffer::new(),
            kupdate: None,
            write_waker: None,
            read_waker: None,
            close_waker: None,
        }));

        let kupdate = KcpUpdate {
            kcore: kcore.clone(),
            kupdate: None,
        };

        drop(std::mem::replace(
            &mut kcore.lock()?.kupdate,
            Some(executor.spawn(async move {
                drop(kupdate.await);
                log::debug!("stop update conv={}", conv);
                clean_callback(conv).await;
                Ok(())
            })),
        ));

        Ok(Self {
            conv,
            target,
            kcore: kcore.clone(),
        })
    }

    fn force_close(&self) -> crate::Result<()> {
        self.kcore.lock()?.close()
    }

    async fn safe_close(&self) -> crate::Result<()> {
        self.kcore.lock()?.close()
    }

    fn stream(
        &self,
        local_addr: Address,
        peer_addr: Address,
        close_callback: Callback,
    ) -> KcpStream<C> {
        KcpStream::new(
            self.conv,
            local_addr,
            peer_addr,
            self.kcore.clone(),
            close_callback,
        )
    }
}

impl<C, E> KcpListener<C, E>
where
    C: UdpSocket + Send + Sync + Clone + Unpin + 'static,
    E: Executor + Clone + Send + Sync + 'static,
{
    pub fn bind(core: C, executor: E) -> crate::Result<Self> {
        let manager: Manager<C> = Default::default();

        let core_fut = Box::pin(Self::run_accept(
            core.clone(),
            manager.clone(),
            executor.clone(),
        ));

        Ok(Self {
            core,
            manager,
            executor,
            futures: vec![core_fut],
        })
    }

    async fn run_accept(core: C, manager: Manager<C>, executor: E) -> crate::Result<State<C>> {
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

            let mut sessions = manager.lock().await;

            let sessions = sessions.entry(id).or_default();

            sessions.retain(|_, klife| match klife {
                KLife::Dead(now, _) => now.elapsed().as_secs() < 30,
                _ => true,
            });

            let new_kcp = !sessions.contains_key(&conv);

            if new_kcp {
                let new_session = KLife::Active({
                    Session::new(conv, Some(addr), core.clone(), executor.clone(), {
                        {
                            let manager = manager.clone();
                            move |conv| async move {
                                let mut sessions = manager.lock().await;
                                let sessions = sessions.get_mut(&id);

                                if let Some(sessions) = sessions {
                                    if let Some(session) = sessions.remove(&conv) {
                                        let dead_session = match session {
                                            KLife::Dead(now, session) => KLife::Dead(now, session),
                                            KLife::Active(session) => {
                                                let _ = session.force_close();
                                                KLife::Dead(Instant::now(), session)
                                            }
                                        };

                                        sessions.insert(conv, dead_session);
                                    }
                                }

                                ()
                            }
                        }
                    })?
                });

                sessions.insert(conv, new_session);
            }

            let klife = unsafe { sessions.get_mut(&conv).unwrap_unchecked() };

            match klife {
                KLife::Dead(now, session) => {
                    let _ = session.force_close();

                    if now.elapsed().as_secs() < 30 {
                        if let Some(target) = session.target.as_ref() {
                            core.send_to(target, &third_party::force_close(session.conv))
                                .await?;
                        }
                    }
                }
                KLife::Active(session) => {
                    if let Err(e) = session.input(data).await {
                        log::warn!("failed to input {}", e);
                        if let Err(e) = session.force_close() {
                            log::warn!("failed to close {}", e);
                        };
                    } else if new_kcp {
                        break Ok(State::Open((
                            id,
                            conv,
                            session.clone(),
                            Address::One(Socket::udp(addr)),
                        )));
                    }
                }
            }
        }
    }

    fn make_close_fn(&self, id: u64, conv: u32) -> Callback {
        let manager = self.manager.clone();
        let executor = self.executor.clone();
        let close_callback = move || {
            executor.spawn(async move {
                let mut sessions = manager.lock().await;
                let session = sessions
                    .get_mut(&id)
                    .map_or(None, |sessions| sessions.get_mut(&conv));

                if let Some(session) = session {
                    match session {
                        KLife::Active(session) => {
                            if let Err(e) = session.safe_close().await {
                                log::warn!("failed to close {}", e);
                            }
                        }
                        _ => {}
                    }
                }
            });
        };

        Some(Box::new(close_callback))
    }
}

impl<C, E> NetSocket for KcpListener<C, E>
where
    C: NetSocket,
{
    fn peer_addr(&self) -> crate::Result<crate::Address> {
        self.core
            .peer_addr()
            .map(|addr| addr.with_kind(crate::SocketKind::Kcp))
    }

    fn local_addr(&self) -> crate::Result<crate::Address> {
        self.core
            .local_addr()
            .map(|addr| addr.with_kind(crate::SocketKind::Kcp))
    }
}

impl<C, E> Accepter for KcpListener<C, E>
where
    C: UdpSocket + Send + Clone + Unpin + Sync + 'static,
    E: Executor + Clone + Sync + Unpin + Send + 'static,
{
    type Stream = KcpStream<C>;

    fn poll_accept(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<Self::Stream>> {
        let mut futures = std::mem::replace(&mut self.futures, Default::default());

        while let Some(mut future) = futures.pop() {
            match Pin::new(&mut future).poll(cx) {
                Poll::Ready(Err(e)) => {
                    log::warn!("{}", e)
                }
                Poll::Ready(Ok(State::Close(conv))) => {
                    log::debug!("kcp closed conv={}", conv);
                }
                Poll::Pending => {
                    self.futures.push(future);
                }
                Poll::Ready(Ok(State::Open((id, conv, kcp, peer_addr)))) => {
                    let accept_fut = Self::run_accept(
                        self.core.clone(),
                        self.manager.clone(),
                        self.executor.clone(),
                    );

                    self.futures.push(Box::pin(accept_fut));

                    self.futures.extend(futures);

                    let close_fn = self.make_close_fn(id, conv);

                    return Poll::Ready(Ok(kcp.stream(
                        self.core.local_addr()?,
                        peer_addr,
                        close_fn,
                    )));
                }
            }
        }

        Poll::Pending
    }
}

impl<C> Clone for Session<C> {
    fn clone(&self) -> Self {
        Self {
            conv: self.conv,
            kcore: self.kcore.clone(),
            target: self.target.clone(),
        }
    }
}

impl Default for Increment {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(1)))
    }
}

impl<C, E> KcpConnector<C, E>
where
    C: UdpSocket + Unpin + Clone + Send + Sync + 'static,
    E: Executor + Clone + Sync + Send + 'static,
{
    pub fn new(core: C, executor: E) -> Self {
        let sessions: Arc<Mutex<HashMap<u32, KLife<C>>>> = Default::default();

        let task = { executor.spawn(Self::run_connect(core.clone(), sessions.clone())) };

        Self {
            core,
            task,
            executor,
            sessions,
            increment: Default::default(),
        }
    }

    fn run_connect(
        core: C,
        sessions: Arc<Mutex<HashMap<u32, KLife<C>>>>,
    ) -> BoxedFuture<crate::Result<()>> {
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

                sessions.retain(|_, klife| match klife {
                    KLife::Dead(now, _) => now.elapsed().as_secs() < 30,
                    _ => true,
                });

                if let Some(session) = sessions.get_mut(&conv) {
                    match session {
                        KLife::Active(session) => {
                            if let Err(e) = session.input(buf).await {
                                log::warn!("failed to input {}", e);
                                if let Err(e) = session.force_close() {
                                    log::warn!("force close failed {}", e);
                                }
                            }
                        }
                        KLife::Dead(_, session) => {
                            core.send(&third_party::force_close(session.conv)).await?;
                        }
                    }
                } else {
                    // FIXME 需要处理time_wait
                    core.send(&third_party::force_close(conv)).await?;
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
                log::trace!("current conv={}", next_conv);

                if !sessions.contains_key(&next_conv) {
                    break next_conv;
                }

                next_conv = self.increment.next().await;

                if next_conv == conv {
                    return Err(KcpErr::NoMoreConv.into());
                }
            }
        };

        let sessions = self.sessions.clone();
        let executor = self.executor.clone();

        let session = Session::new(conv, None, self.core.clone(), self.executor.clone(), {
            let sessions = self.sessions.clone();
            move |conv| async move {
                let mut sessions = sessions.lock().await;
                if let Some(session) = sessions.remove(&conv) {
                    let dead_session = match session {
                        KLife::Dead(now, session) => KLife::Dead(now, session),
                        KLife::Active(session) => {
                            let _ = session.force_close();
                            KLife::Dead(Instant::now(), session)
                        }
                    };

                    sessions.insert(conv, dead_session);
                    log::debug!("clean the session conv={}", conv);
                }

                ()
            }
        })?;

        self.sessions
            .lock()
            .await
            .insert(conv, KLife::Active(session.clone()));

        let close_callback = move || {
            executor.spawn(async move {
                log::debug!("closing conv {}", conv);
                if let Some(session) = sessions.lock().await.get_mut(&conv) {
                    match session {
                        KLife::Active(session) => {
                            if let Err(e) = session.safe_close().await {
                                log::warn!("failed to close {}", e);
                            }
                        }
                        _ => {}
                    }
                }
            });
        };

        Ok(session.stream(
            self.core.local_addr()?,
            self.core.peer_addr()?,
            Some(Box::new(close_callback)),
        ))
    }
}

impl Increment {
    pub async fn current(&self) -> u32 {
        *self.0.lock().await
    }

    pub async fn next(&self) -> u32 {
        let mut inc = self.0.lock().await;
        let (_, overflow) = inc.overflowing_add(1);
        *inc = if overflow { 0 } else { *inc + 1 };
        *inc
    }
}

unsafe impl<C> Sync for KcpUpdate<C> {}
unsafe impl<C> Send for KcpUpdate<C> {}

impl<C, E> Drop for KcpConnector<C, E> {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl<C> Future for KcpUpdate<C>
where
    C: UdpSocket + Unpin + 'static,
{
    type Output = crate::Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        loop {
            let state = match self.kupdate.take() {
                Some(inner) => inner,
                None => UpdateState::WaitUpdate({
                    let mut kcore = self.kcore.lock()?;
                    let next = kcore.check(now_mills());

                    if kcore.closed() {
                        kcore.close_waker.take().map(Waker::wake);
                        kcore.write_waker.take().map(Waker::wake);
                        kcore.read_waker.take().map(Waker::wake);
                        return Poll::Ready(Err(KcpErr::ConnectionReset.into()));
                    }

                    log::trace!(
                        "next update {}ms conv={} cls={}",
                        next,
                        kcore.conv(),
                        kcore.close_state()
                    );

                    Box::pin(async move {
                        time::sleep(Duration::from_millis(next as u64)).await;
                        Ok(())
                    })
                }),
            };

            match state {
                UpdateState::Updating(mut fut) => match Pin::new(&mut fut).poll(cx)? {
                    Poll::Pending => {
                        drop(std::mem::replace(
                            &mut self.kupdate,
                            Some(UpdateState::Updating(fut)),
                        ));

                        return Poll::Pending;
                    }
                    _ => {}
                },
                UpdateState::WaitUpdate(mut fut) => match Pin::new(&mut fut).poll(cx)? {
                    Poll::Pending => {
                        drop(std::mem::replace(
                            &mut self.kupdate,
                            Some(UpdateState::WaitUpdate(fut)),
                        ));

                        return Poll::Pending;
                    }
                    Poll::Ready(()) => {
                        let kcore = self.kcore.clone();

                        let fut = async move {
                            let mut kcore = kcore.lock()?;

                            kcore.update(now_mills()).await?;

                            if kcore.peeksize().is_ok() {
                                kcore.read_waker.take().map(Waker::wake);
                            }

                            if kcore.wait_snd() < third_party::KCP_WND_RCV as usize {
                                kcore.write_waker.take().map(Waker::wake);
                            }

                            if kcore.wait_snd() == 0 {
                                kcore.close_waker.take().map(Waker::wake);
                            }

                            if kcore.is_dead_link() {
                                drop(kcore.force_close());
                                kcore.close_waker.take().map(Waker::wake);
                                kcore.write_waker.take().map(Waker::wake);
                                kcore.read_waker.take().map(Waker::wake);
                                Err(KcpErr::IoError(std::io::ErrorKind::TimedOut.into()).into())
                            } else {
                                Ok(())
                            }
                        };

                        drop(std::mem::replace(
                            &mut self.kupdate,
                            Some(UpdateState::Updating(Box::pin(fut))),
                        ));
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        ext::{AsyncReadExt, AsyncWriteExt},
        io, AccepterExt, TokioExecutor,
    };

    use super::{KcpConnector, KcpListener};

    fn init_logger() {
        #[cfg(feature = "fuso-log")]
        env_logger::builder()
            .filter_module("fuso", log::LevelFilter::Debug)
            .init();
    }

    #[test]
    pub fn test_kcp_server() {
        init_logger();

        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let udp = tokio::net::UdpSocket::bind("0.0.0.0:7777").await.unwrap();
                let udp = Arc::new(udp);

                let mut kcp = KcpListener::bind(udp, TokioExecutor).unwrap();

                loop {
                    match kcp.accept().await {
                        Ok(kcp) => {
                            tokio::spawn(async move {
                                let (mut reader, mut writer) = io::split(kcp);

                                loop {
                                    let mut buf = Vec::new();
                                    buf.resize(1500, 0);
                                    let n = reader.read(&mut buf).await.unwrap();
                                    buf.truncate(n);

                                    let s = String::from_utf8_lossy(&buf);

                                    log::debug!("receive  {:?}", s);
                                    if s.contains("close") {
                                        let _ = writer.close().await;
                                        log::debug!("close ....");
                                        break;
                                    }

                                    let mut writer = writer.clone();
                                    tokio::spawn(async move {
                                        log::debug!("write hello world");
                                        let _ = writer.write_all(b"hello world").await;
                                    });
                                }
                            });
                        }
                        Err(_) => {}
                    }
                }
            });
    }

    #[test]
    pub fn test_kcp_client() {
        init_logger();

        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let udp = tokio::net::UdpSocket::bind("0.0.0.0:0").await.unwrap();
                udp.connect("127.0.0.1:7777").await.unwrap();

                let kcp = KcpConnector::new(Arc::new(udp), TokioExecutor);

                let mut kcp = kcp.connect().await.unwrap();

                loop {
                    let mut buf = String::new();
                    std::io::stdin().read_line(&mut buf).unwrap();
                    let _ = kcp.write_all(buf.as_bytes()).await;
                    let mut buf = Vec::new();
                    buf.resize(1500, 0);
                    let n = kcp.read(&mut buf).await.unwrap();
                    buf.truncate(n);
                    log::debug!("{:?}", buf)
                }
            });
    }
}
