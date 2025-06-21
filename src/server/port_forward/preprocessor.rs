use std::{sync::Arc, task::Poll};

use fuso_socks::Socks;

use crate::{
    config::client::{Addr, ServerAddr},
    core::{
        processor::{self, AbstractPreprocessor, Preprocessor, PreprocessorSelector, Selector},
        rpc::structs::port_forward::{Target, VisitorProtocol},
    },
    error,
    server::udp_forward::UdpForwarder,
};

use super::Connection;

#[macro_export]
macro_rules! unpack_socks_addr {
    ($addr: expr) => {
        match $addr {
            fuso_socks::Addr::Socket(ip, port) => (
                $crate::config::client::ServerAddr(vec![$crate::config::client::Addr::WithIpAddr(
                    ip,
                )]),
                port,
            ),
            fuso_socks::Addr::Domain(domain, port) => (
                $crate::config::client::ServerAddr(vec![$crate::config::client::Addr::WithDomain(
                    domain,
                )]),
                port,
            ),
        }
    };
}

pub struct WithPortForwardPreprocessor<'a>(PreprocessorSelector<'a, Connection, VisitorProtocol>);

pub struct Socks5Preprocessor {
    udp_forwarder: Arc<dyn UdpForwarder + Sync + Send + 'static>,
}

impl Preprocessor<Connection> for () {
    type Output = error::Result<VisitorProtocol>;

    fn prepare<'a>(&'a self, input: Connection) -> crate::core::BoxedFuture<'a, Self::Output> {
        Box::pin(async move { Ok(VisitorProtocol::Other(input, None)) })
    }
}

impl Preprocessor<Connection> for Option<()> {
    type Output = error::Result<Connection>;
    fn prepare<'a>(&'a self, input: Connection) -> crate::core::BoxedFuture<'a, Self::Output> {
        Box::pin(async move { Ok(input) })
    }
}

impl futures::AsyncRead for Connection {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        crate::core::io::AsyncRead::poll_read(self, cx, buf).map_err(Into::into)
    }
}

impl futures::AsyncWrite for Connection {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        crate::core::io::AsyncWrite::poll_write(self, cx, buf).map_err(Into::into)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        crate::core::io::AsyncWrite::poll_flush(self, cx).map_err(Into::into)
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl<'p> Preprocessor<Connection> for WithPortForwardPreprocessor<'p> {
    type Output = error::Result<VisitorProtocol>;

    fn prepare<'a>(&'a self, input: Connection) -> crate::core::BoxedFuture<'a, Self::Output> {
        Box::pin(async move {
            match self.0.prepare(input).await? {
                processor::Prepare::Ok(out) => Ok(out),
                processor::Prepare::Bad(input) => Ok(VisitorProtocol::Other(input, None)),
            }
        })
    }
}

impl<'a> From<PreprocessorSelector<'a, Connection, VisitorProtocol>>
    for AbstractPreprocessor<'a, Connection, error::Result<VisitorProtocol>>
{
    fn from(selector: PreprocessorSelector<'a, Connection, VisitorProtocol>) -> Self {
        AbstractPreprocessor(Arc::new(WithPortForwardPreprocessor(selector)))
    }
}

impl Socks5Preprocessor {
    pub fn new<F>(udp_forwarder: F) -> Self
    where
        F: UdpForwarder + Sync + Send + 'static,
    {
        Socks5Preprocessor {
            udp_forwarder: Arc::new(udp_forwarder),
        }
    }
}

impl Preprocessor<Connection> for Socks5Preprocessor {
    type Output = error::Result<Selector<Connection, VisitorProtocol>>;
    fn prepare<'a>(&'a self, input: Connection) -> crate::core::BoxedFuture<'a, Self::Output> {
        Box::pin(async move {
            let mut input = input;

            input.mark();

            Ok({
                match fuso_socks::Socks::parse(&mut input, None).await? {
                    Socks::Invalid => {
                        input.reset();
                        Selector::Next(input)
                    }
                    Socks::Tcp(addr) => {
                        input.discard();
                        let (addr, port) = unpack_socks_addr!(addr);
                        Selector::Accepted(VisitorProtocol::Socks(input, Target::Tcp(addr, port)))
                    }
                    Socks::Udp(addr) => {
                        input.discard();

                        let (addr, port) = unpack_socks_addr!(addr);
                        let conn = self.udp_forwarder.open(input).await?;

                        Selector::Accepted(VisitorProtocol::Socks(conn, Target::Udp(addr, port)))
                    }
                }
            })
        })
    }
}
