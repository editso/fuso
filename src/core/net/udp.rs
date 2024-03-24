use std::{net::SocketAddr, pin::Pin};

use crate::{core::{BoxedFuture, Provider}, error};

pub type BoxedDatagram<'a> = Box<dyn Datagram + Send + Sync + Unpin + 'a>;

pub trait UdpProvider {
    type Binder: Provider<BoxedFuture<'static, error::Result<BoxedDatagram<'static>>>, Arg = SocketAddr>;
    type Connect: Provider<BoxedFuture<'static, error::Result<BoxedDatagram<'static>>>, Arg = SocketAddr>;
}

pub struct UdpSocket<'a> {
    inner: BoxedDatagram<'a>,
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

pub trait Datagram: AsyncRecvfrom + AsyncSendTo + AsyncRecv + AsyncSend {
    fn boxed_clone(&self) -> Box<dyn Datagram + Sync + Send + Unpin>;
}



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
    pub async fn bind<P, A>(addr: A) -> error::Result<Self>
    where
        P: UdpProvider,
        A: Into<SocketAddr>,
    {
        Ok(UdpSocket {
            inner: P::Binder::call(addr.into()).await?,
        })
    }

    pub async fn connect<P, A>(addr: A) -> error::Result<Self>
    where
        P: UdpProvider,
        A: Into<SocketAddr>,
    {
        Ok(UdpSocket {
            inner: P::Connect::call(addr.into()).await?,
        })
    }
}
