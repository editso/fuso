use std::io::Cursor;

use async_trait::async_trait;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures::AsyncReadExt;
use smol::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

use crate::error::{self, Result};

const MAGIC: u32 = 0xFA;

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
    fn spwan_forward(self, to: To) -> Result<()>;
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

#[async_trait]
impl<To> Forward<To> for To
where
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

    #[inline]
    fn spwan_forward(self, to: To) -> Result<()> {
        smol::spawn(async move {
            let ret = Self::forward(self, to).await;

            if ret.is_err() {
                log::warn!("[fuso] Forward failure {}", ret.unwrap_err());
            }
        })
        .detach();
        Ok(())
    }
}
