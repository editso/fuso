use std::{
    io::Cursor,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex, RwLock},
    task::Poll,
};

use async_trait::async_trait;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures::{AsyncReadExt, Future};
use smol::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    Executor,
};

use crate::{
    error::{self, Result},
    Buffer, UdpStream,
};

const MAGIC: u32 = 0xCC;

#[inline]
pub fn now_mills() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[inline]
pub async fn copy<R, W>(mut reader: R, mut writer: W) -> std::io::Result<()>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    loop {
        let mut buf = Vec::new();
        buf.resize(0x2000, 0);

        let n = reader.read(&mut buf).await?;

        if n == 0 {
            let _ = writer.close().await;
            break Ok(());
        }

        buf.truncate(n);

        writer.write_all(&mut buf).await?;
    }
}

#[derive(Debug, Clone)]
pub struct Packet {
    magic: u32,
    cmd: u8,
    len: u32,
    data: Bytes,
}

#[async_trait]
pub trait FusoPacket {
    async fn recv(&mut self) -> Result<Packet>;
    async fn send(&mut self, packet: Packet) -> Result<()>;
}

pub trait FusoStreamEx {
    fn as_fuso_stream(self) -> FusoStream;
}

#[async_trait]
pub trait FusoAuth<IO> {
    async fn auth(&self, io: &mut IO) -> Result<()>;
}

#[async_trait]
pub trait FusoListener<T> {
    async fn accept(&mut self) -> Result<T>;
    async fn close(&mut self) -> Result<()>;
}

#[async_trait]
pub trait Forward<To> {
    async fn forward(self, to: To) -> Result<()>;
}

#[async_trait]
pub trait AsyncTcpSocketEx<B, C> {
    async fn tcp_listen(self) -> Result<B>;
    async fn tcp_connect(self) -> Result<C>;
}

#[derive(Debug, Clone)]
pub struct Rollback<T, Store> {
    target: T,
    back_store: Store,
    begin_store: Store,
    rollback: Arc<RwLock<bool>>,
}

#[derive(Clone)]
pub struct FusoStream {
    peer_addr: Option<SocketAddr>,
    local_addr: Option<SocketAddr>,
    core_reader: Arc<Mutex<dyn AsyncRead + Unpin + Send + Sync + 'static>>,
    core_writer: Arc<Mutex<dyn AsyncWrite + Unpin + Send + Sync + 'static>>,
}

pub trait RollbackEx<T, Store> {
    fn roll(self) -> Rollback<T, Store>;
}

pub trait Spawn {
    fn detach(self);
}

impl Packet {
    #[inline]
    fn constructor(magic: u32, cmd: u8, data: Bytes) -> Self {
        Self {
            magic,
            cmd,
            len: data.len() as u32,
            data,
        }
    }

    #[inline]
    pub fn new(cmd: u8, data: Bytes) -> Self {
        Self::constructor(MAGIC, cmd, data)
    }

    #[inline]
    pub fn size() -> usize {
        // std::mem::size_of::<Self>() - std::mem::size_of::<Bytes>()
        10
    }

    #[inline]
    pub fn magic() -> u32 {
        MAGIC
    }

    #[inline]
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < Self::size() {
            return Err(error::ErrorKind::BadPacket.into());
        }

        let mut packet = Cursor::new(data);

        Ok(Self {
            magic: {
                let magic = packet.get_u32();

                if Self::magic() != magic {
                    log::warn!("decode fuso packet error {:?}", data);
                    return Err(error::ErrorKind::BadPacket.into());
                }

                magic
            },
            cmd: packet.get_u8(),
            len: packet.get_u32(),
            data: Bytes::new(),
        })
    }

    #[inline]
    pub fn decode_data(data: &[u8]) -> Result<Self> {
        let mut packet = Self::decode(data)?;
        let data = &data[Self::size()..];
        if data.len() < packet.len as usize {
            Err(error::ErrorKind::BadPacket.into())
        } else {
            packet.set_data(data[..packet.len as usize].to_vec());
            Ok(packet)
        }
    }

    #[inline]
    pub fn set_data<Data: Into<Bytes>>(&mut self, data: Data) {
        self.data = data.into();
        self.len = self.data.len() as u32;
    }

    #[inline]
    pub fn get_len(&self) -> usize {
        self.len as usize
    }

    #[inline]
    pub fn get_cmd(&self) -> u8 {
        self.cmd
    }

    #[inline]
    pub fn get_data(&self) -> &Bytes {
        &self.data
    }

    #[inline]
    pub fn get_mut_data(&mut self) -> &mut Bytes {
        &mut self.data
    }

    #[inline]
    pub fn encode(self) -> Bytes {
        let mut data = BytesMut::new();

        data.put_u32(self.magic);
        data.put_u8(self.cmd);
        data.put_u32(self.data.len() as u32);

        while data.len() != Self::size() {
            data.put_u8(0);
        }

        data.put_slice(&self.data);

        data.into()
    }
}

impl TryFrom<&[u8]> for Packet {
    type Error = error::Error;

    #[inline]
    fn try_from(data: &[u8]) -> std::result::Result<Self, Self::Error> {
        Self::decode(data)
    }
}

impl From<Packet> for Bytes {
    #[inline]
    fn from(packet: Packet) -> Self {
        packet.encode()
    }
}

impl From<Packet> for Vec<u8> {
    #[inline]
    fn from(packet: Packet) -> Self {
        let bytes = packet.encode();
        bytes.to_vec()
    }
}

impl From<Packet> for BytesMut {
    #[inline]
    fn from(packet: Packet) -> Self {
        let mut data = BytesMut::new();
        data.put_slice(&packet.encode());
        data
    }
}

#[async_trait]
impl<T> FusoPacket for T
where
    T: AsyncRead + AsyncWrite + Unpin + Sync + Send + 'static,
{
    #[inline]
    async fn recv(&mut self) -> Result<Packet> {
        let mut buffer = Vec::new();

        buffer.resize(Packet::size(), 0);

        self.read_exact(&mut buffer)
            .await
            .map_err(|e| error::Error::with_io(e))?;

        let mut packet = Packet::decode(&buffer)?;

        buffer.clear();
        buffer.resize(packet.get_len(), 0);

        self.read_exact(&mut buffer)
            .await
            .map_err(|_| error::Error::new(error::ErrorKind::BadPacket))?;

        packet.set_data(buffer);

        Ok(packet)
    }

    #[inline]
    async fn send(&mut self, packet: Packet) -> Result<()> {
        self.write(&packet.encode())
            .await
            .map_err(|e| error::Error::with_io(e))?;
        Ok(())
    }
}

impl<T> FusoStreamEx for T
where
    T: AsyncWrite + AsyncRead + Unpin + Send + Sync + 'static,
{
    fn as_fuso_stream(self) -> FusoStream {
        let (reader, writer) = self.split();

        FusoStream {
            peer_addr: None,
            local_addr: None,
            core_reader: Arc::new(Mutex::new(reader)),
            core_writer: Arc::new(Mutex::new(writer)),
        }
    }
}

impl<T, A> Spawn for T
where
    A: Send + 'static,
    T: Future<Output = A> + Send + 'static,
{
    #[inline]
    fn detach(self) {
        static GLOBAL: once_cell::sync::Lazy<Executor<'_>> = once_cell::sync::Lazy::new(|| {
            log::info!("spawn thread count {}", num_cpus::get());
            for n in 0..num_cpus::get() {
                log::trace!("spawn executor thread fuso-{}", n);
                std::thread::Builder::new()
                    .name(format!("fuso-{}", n))
                    .spawn(|| loop {
                        std::panic::catch_unwind(|| {
                            smol::block_on(GLOBAL.run(smol::future::pending::<()>()))
                        })
                        .ok();
                    })
                    .expect("cannot spawn executor thread");
            }

            Executor::new()
        });

        GLOBAL.spawn(self).detach();
    }
}

#[async_trait]
impl<From, To> Forward<To> for From
where
    From: AsyncRead + AsyncWrite + Unpin + Send + Sync + 'static,
    To: AsyncRead + AsyncWrite + Unpin + Send + Sync + 'static,
{
    #[inline]
    async fn forward(self, to: To) -> Result<()> {
        let (reader_s, writer_s) = self.split();
        let (reader_t, writer_t) = to.split();

        smol::future::race(copy(reader_t, writer_s), copy(reader_s, writer_t))
            .await
            .map_err(|e| {
                log::debug!("forward {}", e);
                error::Error::with_io(e)
            })?;

        Ok(())
    }
}

#[async_trait]
impl AsyncTcpSocketEx<TcpListener, TcpStream> for SocketAddr {
    #[inline]
    async fn tcp_listen(self) -> Result<TcpListener> {
        TcpListener::bind(self).await.map_err(|e| e.into())
    }

    #[inline]
    async fn tcp_connect(self) -> Result<TcpStream> {
        TcpStream::connect(self).await.map_err(|e| e.into())
    }
}

#[async_trait]
impl AsyncTcpSocketEx<TcpListener, TcpStream> for String {
    #[inline]
    async fn tcp_listen(self) -> Result<TcpListener> {
        TcpListener::bind(self).await.map_err(|e| e.into())
    }

    #[inline]
    async fn tcp_connect(self) -> Result<TcpStream> {
        TcpStream::connect(self).await.map_err(|e| e.into())
    }
}

impl<T> RollbackEx<T, Buffer<u8>> for T
where
    T: AsyncRead + AsyncWrite + Send + Sync + 'static,
{
    #[inline]
    fn roll(self) -> Rollback<T, Buffer<u8>> {
        Rollback {
            target: self,
            rollback: Arc::new(RwLock::new(false)),
            begin_store: Buffer::new(),
            back_store: Buffer::new(),
        }
    }
}

impl<T> Rollback<T, Buffer<u8>> {
    #[inline]
    pub async fn begin(&self) -> Result<()> {
        if self.rollback.read().unwrap().eq(&true) {
            Err("Rollback enabled".into())
        } else {
            *self.rollback.write().unwrap() = true;
            Ok(())
        }
    }

    #[inline]
    pub async fn back(&mut self) -> Result<()> {
        if self.rollback.read().unwrap().eq(&false) {
            Err("Rollback enabled".into())
        } else {
            // 简单实现, 待优化...
            let begin = &mut self.begin_store;
            let back = &mut self.back_store;

            if !begin.is_empty() {
                let mut buf = Vec::new();
                buf.resize(begin.len(), 0);
                begin.read_exact(&mut buf).await?;
                back.push_front(&mut buf);
            }

            *self.rollback.write().unwrap() = false;
            Ok(())
        }
    }

    #[inline]
    pub async fn release(&mut self) -> Result<()> {
        *self.rollback.write().unwrap() = false;
        self.begin_store.clear();
        self.back_store.clear();

        Ok(())
    }
}

impl Rollback<TcpStream, Buffer<u8>> {
    #[inline]
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.target.local_addr()
    }

    #[inline]
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.target.peer_addr()
    }
}

impl Rollback<UdpStream, Buffer<u8>> {
    #[inline]
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.target.local_addr()
    }

    #[inline]
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.target.peer_addr()
    }
}

impl From<Rollback<TcpStream, Buffer<u8>>> for TcpStream {
    #[inline]
    fn from(roll: Rollback<TcpStream, Buffer<u8>>) -> Self {
        roll.target
    }
}

#[async_trait]
impl<T> AsyncRead for Rollback<T, Buffer<u8>>
where
    T: AsyncRead + Unpin + Send + Sync + 'static,
{
    #[inline]
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let rollback = *self.rollback.read().unwrap();

        let read_len = if !self.back_store.is_empty() {
            match Pin::new(&mut self.back_store).poll_read(cx, buf)? {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(n) => n,
            }
        } else {
            0
        };

        let poll = if read_len >= buf.len() {
            std::task::Poll::Ready(Ok(read_len))
        } else {
            match Pin::new(&mut self.target).poll_read(cx, &mut buf[read_len..])? {
                std::task::Poll::Pending => Poll::Pending,
                std::task::Poll::Ready(0) => Poll::Ready(Ok(read_len)),
                std::task::Poll::Ready(n) => Poll::Ready(Ok(read_len + n)),
            }
        };

        match poll {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => Poll::Ready(e),
            Poll::Ready(Ok(n)) => {
                if rollback {
                    match Pin::new(&mut self.begin_store).poll_write(cx, &buf[..n])? {
                        Poll::Pending => {
                            log::warn!("[store] Shouldn't be executed to this point");
                        }
                        Poll::Ready(n) => {
                            log::debug!("[store] save rollback {}", n);
                        }
                    }
                }
                Poll::Ready(Ok(n))
            }
        }
    }
}

#[async_trait]
impl<T> AsyncWrite for Rollback<T, Buffer<u8>>
where
    T: AsyncWrite + Unpin + Send + Sync + 'static,
{
    #[inline]
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        *self.rollback.write().unwrap() = false;
        self.begin_store.clear();
        self.back_store.clear();

        Pin::new(&mut self.target).poll_write(cx, buf)
    }

    #[inline]
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.target).poll_flush(cx)
    }

    #[inline]
    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.target).poll_close(cx)
    }
}

impl AsyncRead for FusoStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        let mut core = self.core_reader.lock().unwrap();
        Pin::new(&mut *core).poll_read(cx, buf)
    }
}

impl AsyncWrite for FusoStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let mut core = self.core_writer.lock().unwrap();
        Pin::new(&mut *core).poll_write(cx, buf)
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        let mut core = self.core_writer.lock().unwrap();
        Pin::new(&mut *core).poll_flush(cx)
    }

    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        let mut core = self.core_writer.lock().unwrap();
        Pin::new(&mut *core).poll_close(cx)
    }
}
