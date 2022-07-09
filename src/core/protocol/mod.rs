#[cfg(feature = "fuso-serde")]
mod serde;
#[cfg(feature = "fuso-serde")]
pub use self::serde::*;

#[cfg(not(feature = "fuso-serde"))]
mod local;

#[cfg(not(feature = "fuso-serde"))]
pub use local::*;

pub mod proto;
pub use proto::*;

use std::{future::Future, pin::Pin, task::Poll};

use crate::{r#async::ReadBuf, AsyncRead, AsyncWrite, PacketErr, Result};

/// f => 0x66
/// u => 0x75
/// s => 0x73
/// o => 0x6f
pub const MAGIC: [u8; 4] = [0x66, 0x75, 0x73, 0x6f];

pub fn head_size() -> usize {
    8
}

pub fn good_packet(magic_buf: &[u8; 4]) -> bool {
    MAGIC.eq(magic_buf)
}

pub fn empty_packet() -> Packet {
    Packet {
        magic: u32::from_be_bytes(MAGIC),
        data_len: 0,
        payload: vec![],
    }
}

pub fn make_packet(buf: Vec<u8>) -> Packet {
    Packet {
        magic: u32::from_be_bytes(MAGIC),
        data_len: buf.len() as u32,
        payload: buf,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    None,
    Head,
    Body,
}

#[pin_project::pin_project]
pub struct RecvPacket<'a, R>
where
    R: Unpin,
{
    buf: Vec<u8>,
    state: State,
    offset: usize,
    #[pin]
    reader: &'a mut R,
}

#[pin_project::pin_project]
pub struct SendPacket<'a, W>
where
    W: Unpin,
{
    buf: &'a [u8],
    offset: usize,
    #[pin]
    writer: &'a mut W,
}

pub trait AsyncRecvPacket: AsyncRead {
    fn recv_packet<'a>(&'a mut self) -> RecvPacket<'a, Self>
    where
        Self: Unpin + Sized,
    {
        RecvPacket {
            buf: Vec::new(),
            state: State::None,
            offset: 0,
            reader: self,
        }
    }
}

pub trait AsyncSendPacket: AsyncWrite {
    fn send_packet<'a>(&'a mut self, buf: &'a [u8]) -> SendPacket<'a, Self>
    where
        Self: Unpin + Sized,
    {
        SendPacket {
            buf,
            offset: 0,
            writer: self,
        }
    }
}

impl<T> AsyncSendPacket for T where T: AsyncWrite + Unpin {}

impl<T> AsyncRecvPacket for T where T: AsyncRead + Unpin {}

impl<'a, T> Future for SendPacket<'a, T>
where
    T: AsyncWrite + Unpin,
{
    type Output = Result<()>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut this = self.project();
        let offset = this.offset;
        loop {
            match Pin::new(&mut **this.writer).poll_write(cx, &this.buf[*offset..]) {
                Poll::Pending => break Poll::Pending,
                Poll::Ready(Err(e)) => break Poll::Ready(Err(e)),
                Poll::Ready(Ok(n)) => {
                    *offset += n;
                }
            };

            if *offset == this.buf.len() {
                break Poll::Ready(Ok(()));
            }
        }
    }
}

impl<'a, T> Future for RecvPacket<'a, T>
where
    T: AsyncRead + Unpin,
{
    type Output = Result<Packet>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut this = self.project();
        let buf = this.buf;
        let offset = this.offset;

        loop {
            match this.state {
                State::None => {
                    if *offset == head_size() {
                        log::trace!("packet header received");
                        drop(std::mem::replace(this.state, State::Head));
                        continue;
                    } else if buf.is_empty() {
                        log::trace!("initialize the header buffer");
                        *offset = 0;
                        buf.resize(head_size(), 0);
                    }
                }
                State::Head if *offset == buf.len() => unsafe {
                    #[allow(unused)]
                    #[repr(C)]
                    struct Head {
                        magic: [u8; 4],
                        data_len: u32,
                    }

                    let head = (buf.as_ptr() as *const Head).as_ref().unwrap();

                    if !self::good_packet(&head.magic) {
                        log::debug!("received illegal package {:x?}", head.magic);
                        break Poll::Ready(Err(PacketErr::Head(head.magic).into()));
                    }

                    let len = head.data_len;

                    log::trace!("received legal packet, data size {}bytes", len);

                    if len == 0 {
                        break Poll::Ready(Ok(self::empty_packet()));
                    }

                    drop(std::mem::replace(buf, {
                        let mut buf = Vec::with_capacity(len as usize);
                        buf.set_len(len as usize);
                        buf
                    }));

                    *offset = 0;
                    drop(std::mem::replace(this.state, State::Body));
                },
                State::Body if *offset == buf.len() => {
                    log::trace!("packet reception completed {}bytes", buf.len());
                    break Poll::Ready(Ok(make_packet(std::mem::replace(buf, Default::default()))));
                }
                _ => {
                    log::trace!(
                        "total {}bytes, received {}bytes remaining {}bytes",
                        buf.len(),
                        offset,
                        buf.len() - *offset
                    );
                }
            }

            let mut buf = ReadBuf::new(&mut buf[*offset..]);

            match Pin::new(&mut **this.reader).poll_read(cx, &mut buf) {
                Poll::Ready(Err(e)) => break Poll::Ready(Err(e)),
                Poll::Pending => break Poll::Pending,
                Poll::Ready(Ok(0)) => {
                    break Poll::Ready(Err({
                        Into::<std::io::Error>::into(std::io::ErrorKind::ConnectionReset).into()
                    }))
                }
                Poll::Ready(Ok(n)) => {
                    *offset += n;
                }
            }
        }
    }
}
