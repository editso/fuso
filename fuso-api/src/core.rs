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

#[async_trait]
pub trait FusoEncoder<OUT> {
    async fn encode(&self) -> Result<OUT>;
}

#[async_trait]
pub trait FusoDecoder<IN, OUT> {
    async fn decode(data: &IN) -> Result<OUT>;
}

#[async_trait]
pub trait FusoAuth {
    async fn auth(&self) -> Result<()>;
}

#[async_trait]
pub trait FusoListener<Stream> {
    async fn accept(&mut self) -> Result<Stream>;
    async fn close(&mut self) -> Result<()>;
}

#[async_trait]
pub trait Forward<To> {
    async fn forward(self, to: To) -> Result<()>;
}

#[async_trait]
pub trait Life<C> {
    async fn start(&self, cx: C) -> Result<()>;
    async fn stop(self) -> Result<()>;
}

#[async_trait]
pub trait AsyncTcpSocketEx<B, C> {
    async fn tcp_listen(self) -> Result<B>;
    async fn tcp_connect(self) -> Result<C>;
}

#[derive(Debug, Clone)]
pub struct Rollback<T, Store> {
    target: Arc<Mutex<T>>,
    rollback: Arc<RwLock<bool>>,
    store: Arc<Mutex<Store>>,
}

pub trait RollbackEx<T, Store> {
    fn roll(self) -> Rollback<T, Store>;
}

pub trait Spwan {
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
        std::mem::size_of::<Self>() - std::mem::size_of::<Bytes>()
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
        if packet.len < data.len() as u32 {
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

    pub fn get_data(&self) -> &Bytes {
        &self.data
    }

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

impl<T, A> Spwan for T
where
    A: Send + 'static,
    T: Future<Output = A> + Send + 'static,
{
    fn detach(self) {
        smol::spawn(self).detach();
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

        smol::future::race(
            smol::io::copy(reader_s, writer_t),
            smol::io::copy(reader_t, writer_s),
        )
        .await
        .map_err(|e| error::Error::with_io(e))?;

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

impl<T> RollbackEx<T, Buffer<u8>> for T
where
    T: AsyncRead + AsyncWrite + Send + Sync + 'static,
{
    fn roll(self) -> Rollback<T, Buffer<u8>> {
        Rollback {
            target: Arc::new(Mutex::new(self)),
            rollback: Arc::new(RwLock::new(false)),
            store: Arc::new(Mutex::new(Buffer::new())),
        }
    }
}

impl<T> Rollback<T, Buffer<u8>> {
    #[inline]
    pub async fn begin(&self) -> Result<()> {
        if self.rollback.read().unwrap().eq(&true) {
            Err("Rollbabck enabled".into())
        } else {
            *self.rollback.write().unwrap() = true;
            Ok(())
        }
    }

    #[inline]
    pub async fn back(&self) -> Result<()> {
        if self.rollback.read().unwrap().eq(&false) {
            Err("Rollbabck enabled".into())
        } else {
            *self.rollback.write().unwrap() = false;
            Ok(())
        }
    }
}

impl Rollback<TcpStream, Buffer<u8>> {
    #[inline]
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.target.lock().unwrap().local_addr()
    }

    #[inline]
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.target.lock().unwrap().peer_addr()
    }
}

impl Rollback<UdpStream, Buffer<u8>> {
    #[inline]
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.target.lock().unwrap().local_addr()
    }

    #[inline]
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.target.lock().unwrap().peer_addr()
    }
}

impl From<Rollback<TcpStream, Buffer<u8>>> for TcpStream {
    #[inline]
    fn from(roll: Rollback<TcpStream, Buffer<u8>>) -> Self {
        roll.target.lock().unwrap().clone()
    }
}

#[async_trait]
impl<T> AsyncRead for Rollback<T, Buffer<u8>>
where
    T: AsyncRead + Unpin + Send + Sync + 'static,
{
    #[inline]
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let rollback = *self.rollback.read().unwrap();

        let read_len = if !rollback {
            let mut io = self.store.lock().unwrap();
            match Pin::new(&mut *io).poll_read(cx, buf)? {
                std::task::Poll::Pending => 0,
                std::task::Poll::Ready(n) => n,
            }
        } else {
            0
        };

        if read_len >= buf.len() {
            std::task::Poll::Ready(Ok(read_len))
        } else {
            let mut io = self.target.lock().unwrap();
            match Pin::new(&mut *io).poll_read(cx, &mut buf[read_len..])? {
                std::task::Poll::Pending => Poll::Pending,
                std::task::Poll::Ready(0) => Poll::Ready(Ok(read_len)),
                std::task::Poll::Ready(n) => {
                    if rollback {
                        let mut store = self.store.lock().unwrap();
                        match Pin::new(&mut *store).poll_write(cx, &buf[..n + read_len])? {
                            Poll::Pending => {
                                log::warn!("[store] Shouldn't be executed to this point");
                            }
                            Poll::Ready(n) => {
                                log::debug!("[store] save rollback {}", n);
                            }
                        }
                    }

                    Poll::Ready(Ok(read_len + n))
                }
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
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let mut io = self.target.lock().unwrap();
        Pin::new(&mut *io).poll_write(cx, buf)
    }

    #[inline]
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let mut io = self.target.lock().unwrap();
        Pin::new(&mut *io).poll_flush(cx)
    }

    #[inline]
    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let mut io = self.target.lock().unwrap();
        Pin::new(&mut *io).poll_close(cx)
    }
}
