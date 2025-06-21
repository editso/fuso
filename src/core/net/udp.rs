use std::{net::SocketAddr, pin::Pin};

use crate::{
    core::{BoxedFuture, Provider},
    error,
};

pub type AbstractDatagram<'a> = Box<dyn Datagram + Send + Sync + Unpin + 'a>;

pub trait UdpProvider {
    type Binder: Provider<
        BoxedFuture<'static, error::Result<(SocketAddr, AbstractDatagram<'static>)>>,
        Arg = SocketAddr,
    >;
    type Connect: Provider<
        BoxedFuture<'static, error::Result<(SocketAddr, AbstractDatagram<'static>)>>,
        Arg = SocketAddr,
    >;
}

pub struct UdpSocket<'a> {
    inner: AbstractDatagram<'a>,
}

#[pin_project::pin_project]
pub struct UdpSend<'a, U> {
    #[pin]
    udp: &'a mut U,
    data: &'a [u8],
}

#[pin_project::pin_project]
pub struct UdpRecv<'a, U> {
    #[pin]
    udp: &'a mut U,
    buf: &'a mut [u8],
}

#[pin_project::pin_project]
pub struct UdpSendTo<'a, U> {
    #[pin]
    udp: &'a mut U,
    addr: &'a SocketAddr,
    data: &'a [u8],
}

#[pin_project::pin_project]
pub struct UdpRecvFrom<'a, U> {
    #[pin]
    udp: &'a mut U,
    buf: &'a mut [u8],
}

pub trait AsyncRecvfrom {
    fn poll_recvfrom(
        &mut self,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<(std::net::SocketAddr, usize)>>;
}

pub trait AsyncSendTo {
    fn poll_sendto(
        &mut self,
        cx: &mut std::task::Context<'_>,
        addr: &std::net::SocketAddr,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>>;
}

pub trait AsyncRecv {
    fn poll_recv(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>>;
}

pub trait AsyncSend {
    fn poll_send(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>>;
}

pub trait AsyncSendExt: AsyncSend {
    fn send<'a>(&'a mut self, data: &'a [u8]) -> UdpSend<'a, Self>
    where
        Self: Sized,
    {
        UdpSend { udp: self, data }
    }
}

pub trait AsyncSendToExt: AsyncSendTo {
    fn send_to<'a>(&'a mut self, addr: &'a SocketAddr, data: &'a [u8]) -> UdpSendTo<'a, Self>
    where
        Self: Sized,
    {
        UdpSendTo {
            udp: self,
            data,
            addr,
        }
    }
}

pub trait AsyncRecvExt: AsyncRecv {
    fn recv<'a>(&'a mut self, buf: &'a mut [u8]) -> UdpRecv<'a, Self>
    where
        Self: Sized,
    {
        UdpRecv { udp: self, buf }
    }
}

pub trait AsyncRecvFromExt: AsyncRecvfrom {
    fn recvfrom<'a>(&'a mut self, buf: &'a mut [u8]) -> UdpRecvFrom<'a, Self>
    where
        Self: Sized,
    {
        UdpRecvFrom { udp: self, buf }
    }
}

pub trait Datagram: AsyncRecvfrom + AsyncSendTo + AsyncRecv + AsyncSend {
    fn boxed_clone(&self) -> Box<dyn Datagram + Sync + Send + Unpin>;
}

impl<T> AsyncRecvExt for T where T: AsyncRecv + Unpin {}
impl<T> AsyncSendExt for T where T: AsyncSend + Unpin {}
impl<T> AsyncSendToExt for T where T: AsyncSendTo + Unpin {}
impl<T> AsyncRecvFromExt for T where T: AsyncRecvfrom + Unpin {}

impl<'a> kcp_rust::AsyncRecvfrom for UdpSocket<'a> {
    fn poll_recvfrom(
        &mut self,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<(std::net::SocketAddr, usize)>> {
        AsyncRecvfrom::poll_recvfrom(self, cx, buf)
    }
}

impl<'a> kcp_rust::AsyncSendTo for UdpSocket<'a> {
    fn poll_sendto(
        &mut self,
        cx: &mut std::task::Context<'_>,
        addr: &std::net::SocketAddr,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        AsyncSendTo::poll_sendto(self, cx, addr, buf)
    }
}

impl<'a> kcp_rust::AsyncRecv for UdpSocket<'a> {
    fn poll_recv(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        AsyncRecv::poll_recv(self, cx, buf)
    }
}

impl<'a> kcp_rust::AsyncSend for UdpSocket<'a> {
    fn poll_send(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        AsyncSend::poll_send(self, cx, buf)
    }
}

impl<'a> AsyncRecvfrom for UdpSocket<'a> {
    fn poll_recvfrom(
        &mut self,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<(std::net::SocketAddr, usize)>> {
        Box::pin(&mut *self.inner).poll_recvfrom(cx, buf)
    }
}

impl<'a> AsyncSendTo for UdpSocket<'a> {
    fn poll_sendto(
        &mut self,
        cx: &mut std::task::Context<'_>,
        addr: &std::net::SocketAddr,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        self.inner.poll_sendto(cx, addr, buf)
    }
}

impl<'a> AsyncRecv for UdpSocket<'a> {
    fn poll_recv(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        Pin::new(&mut *self.inner).poll_recv(cx, buf)
    }
}

impl<'a> AsyncSend for UdpSocket<'a> {
    fn poll_send(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        Pin::new(&mut *self.inner).poll_send(cx, buf)
    }
}

impl<'a> Clone for UdpSocket<'a> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.boxed_clone(),
        }
    }
}

impl<'a> UdpSocket<'a> {
    pub fn new(udp: AbstractDatagram<'a>) -> Self {
        Self { inner: udp }
    }

    pub async fn bind<P, A>(addr: A) -> error::Result<(SocketAddr, Self)>
    where
        P: UdpProvider,
        A: Into<SocketAddr>,
    {
        let (addr, udp) = P::Binder::call(addr.into()).await?;
        Ok((addr, UdpSocket { inner: udp }))
    }

    pub async fn connect<P, A>(addr: A) -> error::Result<(SocketAddr, Self)>
    where
        P: UdpProvider,
        A: Into<SocketAddr>,
    {
        let (addr, udp) = P::Connect::call(addr.into()).await?;
        Ok((addr, UdpSocket { inner: udp }))
    }
}

impl<'a, U> std::future::Future for UdpSend<'a, U>
where
    U: AsyncSend + Unpin,
{
    type Output = error::Result<usize>;
    fn poll(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut this = self.project();
        Pin::new(&mut **this.udp)
            .poll_send(cx, this.data)
            .map_err(Into::into)
    }
}

impl<'a, U> std::future::Future for UdpRecv<'a, U>
where
    U: AsyncRecv + Unpin,
{
    type Output = error::Result<usize>;
    fn poll(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut this = self.project();
        Pin::new(&mut **this.udp)
            .poll_recv(cx, this.buf)
            .map_err(Into::into)
    }
}

impl<'a, U> std::future::Future for UdpSendTo<'a, U>
where
    U: AsyncSendTo + Unpin,
{
    type Output = error::Result<usize>;
    fn poll(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut this = self.project();
        Pin::new(&mut **this.udp)
            .poll_sendto(cx, &this.addr, this.data)
            .map_err(Into::into)
    }
}

impl<'a, U> std::future::Future for UdpRecvFrom<'a, U>
where
    U: AsyncRecvfrom + Unpin,
{
    type Output = error::Result<(SocketAddr, usize)>;
    fn poll(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut this = self.project();
        Pin::new(&mut **this.udp)
            .poll_recvfrom(cx, this.buf)
            .map_err(Into::into)
    }
}
