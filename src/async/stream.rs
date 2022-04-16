use std::{future::Future, pin::Pin};

use crate::Result;

pub struct Connect<'a, C: Unpin> {
    connect: &'a mut C,
}

pub struct Close<'a, C: Unpin> {
    close: &'a mut C,
}

pub struct Accepter<'a, A: Unpin> {
    accepter: &'a mut A,
}

#[pin_project::pin_project]
pub struct Register<'a, R: Unpin, M> {
    metadata: &'a M,
    #[pin]
    register: &'a mut R,
}

#[pin_project::pin_project]
pub struct EncryptRead<'a, R: Unpin> {
    buf: &'a mut [u8],
    #[pin]
    reader: &'a mut R,
}

#[pin_project::pin_project]
pub struct EncryptWrite<'a, W: Unpin> {
    buf: &'a [u8],
    #[pin]
    writer: &'a mut W,
}

pub trait AsyncProxy: crate::traits::Proxy {
    fn connect<'a>(&'a mut self) -> Connect<'a, Self>
    where
        Self: Unpin + Sized,
    {
        Connect { connect: self }
    }

    fn close<'a>(&'a mut self) -> Close<'a, Self>
    where
        Self: Unpin + Sized,
    {
        Close { close: self }
    }
}

pub trait AsyncAccepter: crate::traits::Accepter {
    fn accept<'a>(&'a mut self) -> Accepter<'a, Self>
    where
        Self: Unpin + Sized,
    {
        Accepter { accepter: self }
    }
}

pub trait AsyncRegister: crate::traits::Register {
    fn register<'a>(
        &'a mut self,
        metadata: &'a Self::Metadata,
    ) -> Register<'a, Self, Self::Metadata>
    where
        Self: Unpin + Sized,
    {
        Register {
            register: self,
            metadata,
        }
    }
}

pub trait AsyncCrypt: crate::traits::Decrypt + crate::traits::Decrypt {
    fn encrypt_read<'a>(&'a mut self, buf: &'a mut [u8]) -> EncryptRead<'a, Self>
    where
        Self: Sized + Unpin,
    {
        EncryptRead { buf, reader: self }
    }

    fn encrypt_write<'a>(&'a mut self, buf: &'a [u8]) -> EncryptWrite<'a, Self>
    where
        Self: Sized + Unpin,
    {
        EncryptWrite { buf, writer: self }
    }
}

impl<'a, A> Future for Accepter<'a, A>
where
    A: crate::traits::Accepter + Unpin,
{
    type Output = Result<A::Stream>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Pin::new(&mut *self.accepter).poll_accept(cx)
    }
}

impl<'a, R> Future for Register<'a, R, R::Metadata>
where
    R: crate::traits::Register + Unpin,
{
    type Output = Result<R::Output>;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut this = self.project();
        Pin::new(&mut **this.register).poll_register(cx, this.metadata)
    }
}

impl<'a, R> Future for EncryptRead<'a, R>
where
    R: crate::traits::Decrypt + Unpin,
{
    type Output = Result<usize>;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut this = self.project();
        Pin::new(&mut **this.reader).poll_decrypt_read(cx, this.buf)
    }
}

impl<'a, W> Future for EncryptWrite<'a, W>
where
    W: crate::traits::Encrypt + Unpin,
{
    type Output = Result<()>;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut this = self.project();
        Pin::new(&mut **this.writer).poll_encrypt_write(cx, this.buf)
    }
}

impl<'a, C> Future for Connect<'a, C>
where
    C: crate::traits::Proxy + Unpin,
{
    type Output = Result<C::Stream>;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Pin::new(&mut *self.connect).poll_connect(cx)
    }
}

impl<'a, C> Future for Close<'a, C>
where
    C: crate::traits::Proxy + Unpin,
{
    type Output = Result<()>;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Pin::new(&mut *self.close).poll_close(cx)
    }
}

impl<T> AsyncAccepter for T where T: crate::traits::Accepter + Unpin {}
impl<T> AsyncCrypt for T where T: crate::traits::Encrypt + crate::traits::Decrypt + Unpin {}
impl<T> AsyncRegister for T where T: crate::traits::Register + Unpin {}
impl<T> AsyncProxy for T where T: crate::traits::Proxy + Unpin {}
