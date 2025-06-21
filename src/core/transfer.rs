use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use parking_lot::Mutex;

use crate::{core::future::Select, error};

use super::Stream;

#[pin_project::pin_project]
pub struct TransmitSend<'t, T> {
    data: &'t [u8],
    #[pin]
    transmitter: &'t mut T,
}

#[pin_project::pin_project]
pub struct TransmitSendAll<'t, T> {
    pos: usize,
    data: &'t [u8],
    #[pin]
    transmitter: &'t mut T,
}

#[pin_project::pin_project]
pub struct TransmitRecv<'t, T> {
    buf: &'t mut [u8],
    #[pin]
    transmitter: &'t mut T,
}

pub struct TransmitWriter<T> {
    transmitter: Arc<Mutex<T>>,
}

pub struct TransmitterReader<T> {
    transmitter: Arc<Mutex<T>>,
}

pub trait Transmitter: Unpin {
    fn poll_send(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<error::Result<usize>>;

    fn poll_recv(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<error::Result<usize>>;
}

pub trait TransmitterExt: Transmitter {
    fn recv<'a>(&'a mut self, buf: &'a mut [u8]) -> TransmitRecv<'a, Self>
    where
        Self: Sized,
    {
        TransmitRecv {
            buf,
            transmitter: self,
        }
    }

    fn send<'a>(&'a mut self, data: &'a [u8]) -> TransmitSend<'a, Self>
    where
        Self: Sized,
    {
        TransmitSend {
            data,
            transmitter: self,
        }
    }

    fn send_all<'a>(&'a mut self, data: &'a [u8]) -> TransmitSendAll<'a, Self>
    where
        Self: Sized,
    {
        TransmitSendAll {
            pos: 0,
            data,
            transmitter: self,
        }
    }

    fn split(self) -> (TransmitWriter<Self>, TransmitterReader<Self>)
    where
        Self: Sized,
    {
        let this = Arc::new(Mutex::new(self));

        (
            TransmitWriter {
                transmitter: this.clone(),
            },
            TransmitterReader { transmitter: this },
        )
    }

    fn transfer<'a, T>(self, to: T) -> Select<'a, error::Result<()>>
    where
        T: Transmitter + Send + 'a,
        Self: Sized + Send + 'a,
    {
        let (s1_writer, s1_reader) = to.split();
        let (s2_writer, s2_reader) = self.split();

        let mut select = Select::new();

        select.add(crate::core::io::copy(s1_writer, s2_reader));
        select.add(crate::core::io::copy(s2_writer, s1_reader));

        select
    }
}

impl<T> TransmitterExt for T where T: Transmitter {}

pub struct AbstractTransmitter<'transmitter> {
    inner: Box<dyn Transmitter + Send + Unpin + 'transmitter>,
}

impl<'transmitter> Transmitter for AbstractTransmitter<'transmitter> {
    fn poll_send(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<error::Result<usize>> {
        Pin::new(&mut *self.inner).poll_send(ctx, buf)
    }

    fn poll_recv(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<error::Result<usize>> {
        Pin::new(&mut *self.inner).poll_recv(ctx, buf)
    }
}

impl<T> Transmitter for T
where
    T: Stream + Unpin,
{
    fn poll_send(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<error::Result<usize>> {
        T::poll_write(self, ctx, buf)
    }

    fn poll_recv(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<error::Result<usize>> {
        T::poll_read(self, ctx, buf)
    }
}

impl<'transmitter, S> From<S> for AbstractTransmitter<'transmitter>
where
    S: Stream + Send + Unpin + 'transmitter,
{
    fn from(value: S) -> Self {
        Self {
            inner: Box::new(value),
        }
    }
}

impl<'t, T> std::future::Future for TransmitSend<'t, T>
where
    T: Transmitter,
{
    type Output = error::Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        Pin::new(&mut **this.transmitter).poll_send(cx, &this.data)
    }
}

impl<'t, T> std::future::Future for TransmitSendAll<'t, T>
where
    T: Transmitter,
{
    type Output = error::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        while *this.pos < this.data.len() {
            match Pin::new(&mut **this.transmitter).poll_send(cx, &this.data[*this.pos..])? {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(0) => {
                    return Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe).into()))
                }
                Poll::Ready(n) => {
                    *this.pos += n;
                }
            };
        }

        Poll::Ready(Ok(()))
    }
}

impl<'t, T> std::future::Future for TransmitRecv<'t, T>
where
    T: Transmitter,
{
    type Output = error::Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        Pin::new(&mut **this.transmitter).poll_recv(cx, this.buf)
    }
}

impl<T> crate::core::AsyncRead for TransmitterReader<T>
where
    T: Transmitter + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<error::Result<usize>> {
        let mut this = self.transmitter.lock();
        Pin::new(&mut *this).poll_recv(cx, buf)
    }
}

impl<T> crate::core::AsyncWrite for TransmitWriter<T>
where
    T: Transmitter + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<error::Result<usize>> {
        let mut this = self.transmitter.lock();
        Pin::new(&mut *this).poll_send(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<error::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl<T> Clone for TransmitterReader<T> {
    fn clone(&self) -> Self {
        Self {
            transmitter: self.transmitter.clone(),
        }
    }
}

impl<T> Clone for TransmitWriter<T> {
    fn clone(&self) -> Self {
        Self {
            transmitter: self.transmitter.clone(),
        }
    }
}
