use crate::{ready, ReadBuf, Result};
use std::net::SocketAddr;
use std::{future::Future, pin::Pin};

use std::task::{Context, Poll};

pub trait Accepter {
    type Stream;
    fn poll_accept(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<Self::Stream>>;
}

pub trait UdpSocket {
    fn poll_recv_from(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<SocketAddr>>;

    fn poll_send(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize>>;

    fn poll_recv(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<()>>;

    fn poll_send_to(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        addr: &SocketAddr,
        buf: &[u8],
    ) -> Poll<Result<usize>>;
}

pub struct Accept<'a, T> {
    accepter: &'a mut T,
}

#[pin_project::pin_project]
pub struct RecvFrom<'a, T> {
    buf: ReadBuf<'a>,
    #[pin]
    receiver: &'a mut T,
}

#[pin_project::pin_project]
pub struct UdpSend<'a, T> {
    buf: &'a [u8],
    #[pin]
    sender: &'a mut T,
}

#[pin_project::pin_project]
pub struct UdpRecv<'a, T> {
    buf: ReadBuf<'a>,
    #[pin]
    receiver: &'a mut T,
}

#[pin_project::pin_project]
pub struct SendTo<'a, T> {
    buf: &'a [u8],
    addr: &'a SocketAddr,
    #[pin]
    sender: &'a mut T,
}

pub trait AccepterExt: Accepter {
    fn accept<'a>(&'a mut self) -> Accept<'a, Self>
    where
        Self: Sized + Unpin,
    {
        Accept { accepter: self }
    }
}

pub trait UdpReceiverExt: UdpSocket {
    fn recv_from<'a>(&'a mut self, buf: &'a mut [u8]) -> RecvFrom<'a, Self>
    where
        Self: Sized + Unpin,
    {
        RecvFrom {
            buf: ReadBuf::new(buf),
            receiver: self,
        }
    }

    fn send<'a>(&'a mut self, buf: &'a [u8]) -> UdpSend<'a, Self>
    where
        Self: Sized + Unpin,
    {
        UdpSend { buf, sender: self }
    }

    fn recv<'a>(&'a mut self, buf: &'a mut [u8]) -> UdpRecv<'a, Self>
    where
        Self: Sized + Unpin,
    {
        UdpRecv {
            buf: ReadBuf::new(buf),
            receiver: self,
        }
    }

    fn send_to<'a>(&'a mut self, addr: &'a SocketAddr, buf: &'a [u8]) -> SendTo<'a, Self>
    where
        Self: Sized + Unpin,
    {
        SendTo {
            buf,
            addr,
            sender: self,
        }
    }
}

impl<T> AccepterExt for T where T: Accepter + Unpin {}
impl<T> UdpReceiverExt for T where T: UdpSocket + Unpin {}

impl<'a, T> Future for Accept<'a, T>
where
    T: Accepter + Unpin,
{
    type Output = Result<T::Stream>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Pin::new(&mut *self.accepter).poll_accept(cx)
    }
}

impl<'a, T> Future for RecvFrom<'a, T>
where
    T: UdpSocket + Unpin,
{
    type Output = Result<(usize, SocketAddr)>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        let addr = ready!(Pin::new(&mut **this.receiver).poll_recv_from(cx, this.buf))?;
        Poll::Ready(Ok((this.buf.filled().len(), addr)))
    }
}

impl<'a, T> Future for SendTo<'a, T>
where
    T: UdpSocket + Unpin,
{
    type Output = Result<usize>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        Pin::new(&mut **this.sender).poll_send_to(cx, this.addr, this.buf)
    }
}

impl<'a, T> Future for UdpSend<'a, T>
where
    T: UdpSocket + Unpin,
{
    type Output = Result<usize>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        Pin::new(&mut **this.sender).poll_send(cx, this.buf)
    }
}

impl<'a, T> Future for UdpRecv<'a, T>
where
    T: UdpSocket + Unpin,
{
    type Output = Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        ready!(Pin::new(&mut **this.receiver).poll_recv(cx, this.buf))?;
        Poll::Ready(Ok(this.buf.filled().len()))
    }
}
