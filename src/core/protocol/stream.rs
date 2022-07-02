use std::{future::Future, pin::Pin, task::Poll};

use crate::{r#async::ReadBuf, AsyncRead, AsyncWrite, PacketErr, Result};

use super::{head_size, make_packet, Packet};

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
            log::debug!("{:?} {:?}", offset, &this.buf[*offset..]);

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
                        log::debug!("[protocol] packet header received");
                        drop(std::mem::replace(this.state, State::Head));
                        continue;
                    } else if buf.is_empty() {
                        log::debug!("[protocol] initialize the header buffer");
                        *offset = 0;
                        buf.resize(super::head_size(), 0);
                    }
                }
                State::Head => unsafe {
                    #[allow(unused)]
                    union Head {
                        raw: [u8; 8],
                        magic: [u8; 4],
                        length: u32,
                    }

                    let head = (buf.as_ptr() as *const Head).as_ref().unwrap_unchecked();

                    if !super::good(&head.magic) {
                        log::warn!("[protocol] received illegal package {:x?}", head.magic);
                        break Poll::Ready(Err(PacketErr::Head(head.magic).into()));
                    }

                    log::debug!(
                        "[protocol] received legal packet, data size {}bytes",
                        head.length
                    );

                    if head.length == 0 {
                        break Poll::Ready(Ok(super::empty()));
                    }

                    drop(std::mem::replace(buf, {
                        let mut buf = Vec::with_capacity(head.length as usize);
                        buf.set_len(head.length as usize);
                        buf
                    }));

                    drop(std::mem::replace(this.state, State::Body));
                },
                State::Body if *offset == buf.len() => {
                    break Poll::Ready(Ok(make_packet(std::mem::replace(buf, Default::default()))));
                }
                _ => {
                    log::debug!(
                        "[protocol] total {}bytes, received {}bytes remaining {}bytes",
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
                Poll::Ready(Ok(n)) => {
                    *offset += n;
                }
            }
        }
    }
}
