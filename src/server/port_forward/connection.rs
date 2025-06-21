use std::{marker::PhantomData, net::SocketAddr, pin::Pin, sync::Arc, task::Poll};

use parking_lot::Mutex;

use crate::{
    core::{
        future::LazyFuture,
        io::{AsyncRead, AsyncWrite},
        stream::{CloneableStream, KeepAliveStream},
        AbstractStream,
    },
    error,
    runtime::Runtime,
};

use super::Connection;

#[derive(Clone)]
struct Hot(Arc<Mutex<usize>>);

struct HotStream<'a> {
    nac: Hot,
    conn: AbstractStream<'a>,
}

struct UConnection {
    /// 活跃连接数
    nac: Hot,
    addr: SocketAddr,
    conn: KeepAliveStream,
}

pub struct UdpConnections<R> {
    conns: Arc<Mutex<Vec<UConnection>>>,
    /// 单条连接最大会话数
    max_nac: usize,
    next_update: Arc<Mutex<LazyFuture<'static, ()>>>,
    _marked: PhantomData<R>,
}

impl<R> Default for UdpConnections<R> {
    fn default() -> Self {
        Self {
            conns: Default::default(),
            max_nac: 50,
            next_update: Arc::new(Mutex::new(LazyFuture::new())),
            _marked: PhantomData,
        }
    }
}

impl<R> UdpConnections<R> {
    pub async fn exchange(&self, conn: Connection) -> error::Result<Connection> {
        log::debug!("new udp forward channel {}", conn.addr());

        let mut uc = UConnection {
            nac: Hot(Arc::new(Mutex::new(0))),
            addr: conn.addr().clone(),
            conn: KeepAliveStream::new(CloneableStream::new(AbstractStream::new(conn))),
        };

        let new_conn = uc.apply()?;

        self.conns.lock().push(uc);

        Ok(new_conn)
    }

    pub async fn apply_idle_conn(&self) -> error::Result<Option<Connection>> {
        let mut conns = self.conns.lock();
        let mut retval = None;

        for conn in conns.iter_mut() {
            if conn.nac.value() < self.max_nac {
                retval = Some(conn.apply()?);
                break;
            }
        }

        // 在下一次获取连接时, 始终选择热度最低的那个
        conns.sort_by(|a, b| a.nac.value().cmp(&b.nac.value()));

        Ok(retval)
    }
}

impl UConnection {
    pub fn apply(&mut self) -> error::Result<Connection> {
        self.nac.add();
        Ok(Connection::new(
            self.addr,
            AbstractStream::new(HotStream {
                nac: self.nac.clone(),
                conn: self.conn.duplicate(),
            }),
        ))
    }
}

impl std::future::Future for UConnection {
    type Output = error::Result<()>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Pin::new(&mut self.conn).poll_check(cx)
    }
}

impl<R> std::future::Future for UdpConnections<R>
where
    R: Runtime,
{
    type Output = error::Result<()>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut conns = self.conns.lock();

        conns.retain_mut(|conn| match Pin::new(conn).poll(cx) {
            Poll::Ready(Err(_)) => false,
            _ => true,
        });

        // let time = conns
        //     .iter()
        //     .map(|u| u.conn.next_update())
        //     .min_by(|x, y| x.cmp(y));

        // if let Some(next) = time {
        //     let mut next_update = self.next_update.lock();
        //     next_update.reset();
        //     let _ = next_update.poll(cx, || {
        //         R::sleep(std::time::Duration::from_millis(next as u64))
        //     });
        // }

        std::task::Poll::Pending
    }
}


impl<'a> AsyncRead for HotStream<'a> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> Poll<error::Result<usize>> {
        Pin::new(&mut self.conn).poll_read(cx, buf)
    }
}

impl<'a> AsyncWrite for HotStream<'a> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<error::Result<usize>> {
        Pin::new(&mut self.conn).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<error::Result<()>> {
        Pin::new(&mut self.conn).poll_flush(cx)
    }
}

impl<'a> Drop for HotStream<'a> {
    fn drop(&mut self) {
        self.nac.sub();
    }
}

impl<R> Clone for UdpConnections<R> {
    fn clone(&self) -> Self {
        Self {
            conns: self.conns.clone(),
            max_nac: self.max_nac.clone(),
            next_update: self.next_update.clone(),
            _marked: PhantomData,
        }
    }
}

impl Hot {
    pub fn sub(&self) {
        *self.0.lock() -= 1;
    }

    pub fn add(&self) {
        *self.0.lock() += 1;
    }

    pub fn value(&self) -> usize {
        *self.0.lock()
    }
}
