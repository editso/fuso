use std::{
    io,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll},
};

use crate::error::FusoError;

use super::io::{AsyncRead, AsyncWrite};

const MAGIC: u32 = 0x6675736f;

const F_MAGIC: u8 = 0x1;
const F_SIZE: u8 = 0x2;

pub trait AsyncPacketRead: AsyncRead {
    fn recv_packet<'a>(&'a mut self) -> RecvPacket<'a, Self>
    where
        Self: Sized,
    {
        RecvPacket {
            flags: F_MAGIC | F_SIZE,
            reader: self,
            buffer: Buffer {
                off: 0,
                data: {
                    let mut buf = Vec::new();
                    buf.resize(std::mem::size_of::<u32>(), 0);
                    buf
                },
            },
        }
    }
}

pub trait AsyncPacketSend: AsyncWrite {
    fn send_packet<'a>(&'a mut self, pkt: &'a [u8]) -> SendPacket<'a, Self>
    where
        Self: Sized,
    {
        SendPacket {
            pos: 0,
            writer: self,
            buffer: pkt,
            header: {
                let mut h = [0u8; 8];
                (&mut h[..4]).copy_from_slice(&(MAGIC as u32).to_be_bytes());
                (&mut h[4..]).copy_from_slice(&(pkt.len() as u32).to_be_bytes());
                h
            },
        }
    }
}

impl<T> AsyncPacketRead for T where T: AsyncRead {}
impl<T> AsyncPacketSend for T where T: AsyncWrite {}

pub struct Buffer {
    off: usize,
    data: Vec<u8>,
}

#[pin_project::pin_project]
pub struct RecvPacket<'a, R> {
    #[pin]
    reader: &'a mut R,
    flags: u8,
    buffer: Buffer,
}

#[pin_project::pin_project]
pub struct SendPacket<'a, W> {
    pos: usize,
    #[pin]
    writer: &'a mut W,
    header: [u8; 8],
    buffer: &'a [u8],
}

impl Buffer {
    fn take(&mut self) -> Vec<u8> {
        std::mem::replace(&mut self.data, Default::default())
    }

    fn reset(&mut self, new: usize) {
        self.off = 0;
        self.data = unsafe {
            let mut renew = Vec::with_capacity(new);
            renew.set_len(new);
            renew
        }
    }

    fn poll<R>(
        &mut self,
        cx: &mut Context<'_>,
        mut reader: Pin<&mut R>,
    ) -> Poll<crate::error::Result<()>>
    where
        R: AsyncRead + Unpin,
    {
        while self.off < self.data.len() {
            let off = self.off;
            match Pin::new(&mut *reader).poll_read(cx, &mut self.data[off..])? {
                Poll::Pending => break,
                Poll::Ready(0) => {
                    return Poll::Ready(Err(io::Error::from(io::ErrorKind::UnexpectedEof).into()))
                }
                Poll::Ready(n) => {
                    self.off += n;
                }
            };
        }

        if self.off < self.data.len() {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

impl Deref for Buffer {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        self.data.deref()
    }
}

impl DerefMut for Buffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data.deref_mut()
    }
}

impl<R> std::future::Future for RecvPacket<'_, R>
where
    R: AsyncRead + Unpin,
{
    type Output = crate::error::Result<Vec<u8>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        loop {
            match this.buffer.poll(cx, Pin::new(&mut *this.reader))? {
                Poll::Pending => break Poll::Pending,
                Poll::Ready(()) => {
                    if (*this.flags & F_MAGIC) == F_MAGIC || (*this.flags & F_SIZE) == F_SIZE {
                        let raw = [
                            this.buffer[0],
                            this.buffer[1],
                            this.buffer[2],
                            this.buffer[3],
                        ];

                        let val = u32::from_be_bytes(raw);
                        let renew = {
                            if (*this.flags & F_MAGIC) == F_MAGIC {
                                if val != MAGIC {
                                    return Poll::Ready(Err(FusoError::BadMagic));
                                } else {
                                    *this.flags &= !F_MAGIC;
                                    4
                                }
                            } else {
                                *this.flags &= !F_SIZE;
                                val as usize
                            }
                        };

                        this.buffer.reset(renew);
                    } else {
                        break Poll::Ready(Ok(this.buffer.take()));
                    }
                }
            }
        }
    }
}

impl<W> std::future::Future for SendPacket<'_, W>
where
    W: AsyncWrite + Unpin,
{
    type Output = crate::error::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        while *this.pos < this.buffer.len() + 8 {
            let data = {
                if *this.pos >= std::mem::size_of::<u32>() * 2 {
                    &this.buffer[*this.pos - 8..]
                } else {
                    &this.header[*this.pos..]
                }
            };

            match Pin::new(&mut *this.writer).poll_write(cx, data)? {
                Poll::Pending => break,
                Poll::Ready(0) => {
                    return Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe).into()))
                }
                Poll::Ready(n) => {
                    *this.pos += n;
                }
            }
        }

        if *this.pos < this.buffer.len() {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::core::protocol::{AsyncPacketRead, AsyncPacketSend};

    #[tokio::test]
    async fn test_protocol() -> crate::error::Result<()> {
        let mut cur = std::io::Cursor::new(Vec::<u8>::new());

        let data = b"hello world";

        cur.send_packet(data).await?;

        cur.set_position(0);

        let pkt = cur.recv_packet().await?;

        assert_eq!(&pkt, data);

        Ok(())
    }
}
