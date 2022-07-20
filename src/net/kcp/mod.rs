mod third_party;

use bincode::de;
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
    time::Duration,
};

use async_mutex::Mutex;

use crate::{
    guard::buffer::Buffer, time, Accepter, AsyncRead, AsyncWrite, Executor, Kind, Task,
    UdpReceiverExt, UdpSocket,
};

type BoxedFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

type Manager<C> = Arc<Mutex<HashMap<u64, HashMap<u32, Session<C>>>>>;
type Clean<T> = Arc<std::sync::Mutex<Vec<BoxedFuture<crate::Result<T>>>>>;
type Callback = Option<Box<dyn Fn() + Sync + Send + 'static>>;

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

enum UpdateState {
    Updating(Pin<Box<dyn Future<Output = crate::Result<()>> + 'static>>),
    WaitUpdate(Pin<Box<dyn Future<Output = crate::Result<()>> + 'static>>),
}

pub struct KcpUpdate<C> {
    kcore: Arc<std::sync::Mutex<KcpCore<C>>>,
    kupdate: Option<UpdateState>,
}

#[derive(Debug, Clone)]
pub struct Increment(Arc<Mutex<u32>>);

pub struct Session<C> {
    pub(crate) conv: u32,
    pub(crate) task: Arc<Task<crate::Result<()>>>,
    pub(crate) target: Option<SocketAddr>,
    pub(crate) kcore: Arc<std::sync::Mutex<KcpCore<C>>>,
}

pub struct KcpListener<C, E> {
    pub(crate) core: C,
    pub(crate) manager: Manager<C>,
    pub(crate) futures: Vec<BoxedFuture<crate::Result<State<C>>>>,
    pub(crate) executor: E,
    pub(crate) close_callbacks: Clean<State<C>>,
}

pub struct KcpConnector<C, E> {
    core: C,
    sessions: Arc<Mutex<HashMap<u32, Session<C>>>>,
    executor: E,
    increment: Increment,
    close_callbacks: Arc<std::sync::Mutex<Vec<BoxedFuture<()>>>>,
}

impl<C> Session<C>
where
    C: UdpSocket + Send + Unpin + 'static,
{
    pub async fn input(&mut self, data: Vec<u8>) -> crate::Result<()> {
        let mut kcore = self.kcore.lock()?;
        let mut next_input = &data[..];

        while next_input.len() > third_party::KCP_OVERHEAD {
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
        kcore: Arc<std::sync::Mutex<KcpCore<C>>>,
        close_callback: Callback,
    ) -> Self {
        Self {
            conv,
            kcore,
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

    fn try_wake_close(&mut self) {
        if let Some(waker) = self.close_waker.take() {
            waker.wake();
        }
    }

    // async fn close(&self) -> crate::Result<()> {
    //     self.kcp.lock().await.1.close()
    // }
}

impl<C> Session<C>
where
    C: UdpSocket + Unpin + Send + Sync + 'static,
{
    pub fn new<E>(conv: u32, target: Option<SocketAddr>, output: C, executor: E) -> Self
    where
        E: Executor + Send + 'static,
    {
        let output = KOutput {
            output,
            target: target.clone(),
        };

        let mut kcp = third_party::Kcp::new(conv, output);

        kcp.set_wndsize(512, 521);
        kcp.set_nodelay(true, 20, 2, true);
        kcp.set_maximum_resend_times(10);

        let kcore = Arc::new(std::sync::Mutex::new(KcpCore {
            kcp,
            kbuf: Buffer::new(),
            write_waker: None,
            read_waker: None,
            close_waker: None,
        }));

        let kupdate = KcpUpdate {
            kcore: kcore.clone(),
            kupdate: None,
        };

        Self {
            conv,
            target,
            kcore: kcore.clone(),
            task: Arc::new({ executor.spawn(kupdate) }),
        }
    }

    // pub async fn close(&self) -> crate::Result<()> {
    //     self.kcore.close().await
    // }

    pub fn new_client(&self, close_callback: Callback) -> KcpStream<C> {
        KcpStream::new(self.conv, self.kcore.clone(), close_callback)
    }

    pub fn new_server(&self, close_callback: Callback) -> KcpStream<C> {
        KcpStream::new(self.conv, self.kcore.clone(), close_callback)
    }
}

impl<C, E> KcpListener<C, E>
where
    C: UdpSocket + Send + Sync + Clone + Unpin + 'static,
    E: Executor + Clone + Send + 'static,
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
            close_callbacks: Default::default(),
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

            let mut manager = manager.lock().await;

            let manager = manager.entry(id).or_default();

            let new_kcp = !manager.contains_key(&conv);

            let session = manager
                .entry(conv)
                .or_insert_with(|| Session::new(conv, Some(addr), core.clone(), executor.clone()));

            if let Err(e) = session.input(data).await {
                log::warn!("{}", e);
                manager.remove(&conv);
            } else if new_kcp {
                break Ok(State::Open((id, conv, session.clone())));
            }
        }
    }

    fn make_close_fn(&self, id: u64, conv: u32) -> Callback {
        let manager = self.manager.clone();
        let kcp_close = self.close_callbacks.clone();
        let close_callback = {
            let manager = manager.clone();
            move || match kcp_close.lock() {
                Ok(mut kcp) => {
                    let manager = manager.clone();
                    let close_fut = async move {
                        if let Some(sessions) = manager.lock().await.get_mut(&id) {
                            match sessions.remove(&conv) {
                                Some(session) => {
                                    // let _ = session.close().await;
                                    Ok(State::Close(session))
                                }
                                None => Err(KcpErr::Lock.into()),
                            }
                        } else {
                            Err(KcpErr::Lock.into())
                        }
                    };
                    kcp.push(Box::pin(close_fut));
                }
                Err(e) => {
                    log::warn!("{}", e);
                }
            }
        };

        Some(Box::new(close_callback))
    }
}

impl<C, E> Accepter for KcpListener<C, E>
where
    C: UdpSocket + Send + Clone + Unpin + Sync + 'static,
    E: Executor + Clone + Unpin + Send + 'static,
{
    type Stream = KcpStream<C>;

    fn poll_accept(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<Self::Stream>> {
        let futures = std::mem::replace(&mut self.futures, Default::default());

        let mut futures = futures.into_iter().chain(std::mem::replace(
            &mut *self.close_callbacks.lock()?,
            Default::default(),
        ));

        while let Some(mut future) = futures.next() {
            match Pin::new(&mut future).poll(cx) {
                Poll::Ready(Err(e)) => {
                    log::warn!("{}", e)
                }
                Poll::Ready(Ok(State::Close(kcp))) => {
                    log::debug!("kcp closed conv={}", kcp.conv);
                    drop(kcp)
                }
                Poll::Ready(Ok(State::Open((id, conv, kcp)))) => {
                    let accept_fut = Self::run_accept(
                        self.core.clone(),
                        self.manager.clone(),
                        self.executor.clone(),
                    );

                    self.futures.push(Box::pin(accept_fut));

                    self.futures.extend(futures);

                    let close_fn = self.make_close_fn(id, conv);

                    return Poll::Ready(Ok(kcp.new_server(close_fn)));
                }
                Poll::Pending => {
                    self.futures.push(future);
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
            task: self.task.clone(),
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
    E: Executor + Clone + Send + 'static,
{
    pub fn new(core: C, executor: E) -> Self {
        let this = Self {
            core: core,
            executor: executor.clone(),
            increment: Default::default(),
            sessions: Default::default(),
            close_callbacks: Default::default(),
        };

        executor.spawn(this.safe_connect(0));

        this
    }

    fn safe_connect(&self, _: u32) -> BoxedFuture<crate::Result<()>> {
        let core = self.core.clone();
        let sessions = self.sessions.clone();
        let clean_futures = self.close_callbacks.clone();

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
                        // let _ = session.flush().await;
                        // let _ = session.close().await;
                    }
                }

                drop(sessions);

                let mut futures = match clean_futures.lock() {
                    Err(e) => return Err(e.into()),
                    Ok(mut futures) => std::mem::replace(&mut *futures, Default::default()),
                };

                tokio::spawn(async move {
                    while let Some(futures) = futures.pop() {
                        futures.await;
                    }
                });
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

        let (close_callbacks, sessions) = (self.close_callbacks.clone(), self.sessions.clone());

        let session = Session::new(conv, None, self.core.clone(), self.executor.clone());

        self.sessions.lock().await.insert(conv, session.clone());

        let fut = self.safe_connect(conv);

        let close_callback = move || match close_callbacks.lock() {
            Ok(mut close) => {
                let sessions = sessions.clone();
                let close_fut = async move {
                    if let Some(session) = sessions.lock().await.remove(&conv) {
                        // drop(session.close().await);
                    }
                };

                close.push(Box::pin(close_fut));
            }
            Err(e) => {
                log::warn!("{}", e)
            }
        };

        Ok(session.new_client(Some(Box::new(close_callback))))
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
                    let next = self.kcore.lock()?.check(now_mills());

                    // log::debug!("next update {}ms", next);

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

                            kcore.kcp.update(now_mills()).await?;

                            if kcore.peeksize().is_ok() {
                                kcore.read_waker.take().map(Waker::wake);
                            }

                            if kcore.wait_snd() > 0 {
                                kcore.write_waker.take().map(Waker::wake);
                            }

                            if kcore.wait_snd() == 0 {
                                kcore.close_waker.take().map(Waker::wake);
                            }

                            if kcore.is_dead_link() {
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
        AccepterExt, TokioExecutor, io,
    };

    use super::{KcpConnector, KcpListener};

    fn init_logger() {
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
                        Ok(mut kcp) => {
                            tokio::spawn(async move {

                                let (mut reader, mut writer) = io::split(kcp);

                                loop {
                                    let mut buf = Vec::new();
                                    buf.resize(1500, 0);
                                    let n = reader.read(&mut buf).await.unwrap();
                                    buf.truncate(n);

                                    let mut writer = writer.clone();
                                    tokio::spawn(async move {
                                        let _ = writer.write_all(b"hello world").await;
                                    });

                                    log::debug!("read ... {:?}", String::from_utf8_lossy(&buf));
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
