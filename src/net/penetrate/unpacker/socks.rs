use std::{net::SocketAddr, pin::Pin, sync::Arc};

use crate::{
    ext::{AsyncReadExt, AsyncWriteExt},
    guard::Fallback,
    penetrate::{
        server::{Peer, Visitor},
        Adapter, PenetrateAdapterBuilder,
    },
    protocol::{make_packet, AsyncRecvPacket, AsyncSendPacket, Message, TryToMessage},
    select::Select,
    socks::{self, NoAuthentication, Socks},
    Addr, Factory, FactoryWrapper, Kind, Socket, Stream, UdpReceiverExt, UdpSocket,
};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub struct PenetrateSocksBuilder<E, SF, CF, S> {
    pub(crate) adapter_builder: PenetrateAdapterBuilder<E, SF, CF, S>,
}

pub struct SocksUnpacker<U> {
    pub(crate) udp_factory: Arc<FactoryWrapper<(), (SocketAddr, U)>>,
}

pub struct SocksNoUdpForwardUnpacker;

pub struct SocksClientUdpForward<U>(pub(crate) FactoryWrapper<Addr, (SocketAddr, U)>);

pub struct SocksUdpForward<S, U> {
    udp_factory: Arc<FactoryWrapper<(), (SocketAddr, U)>>,
    guard: std::sync::Mutex<Option<S>>,
}

impl<E, SF, CF, S> PenetrateSocksBuilder<E, SF, CF, S>
where
    S: Stream + Send + Sync + 'static,
{
    pub fn no_udp_forward(mut self) -> PenetrateAdapterBuilder<E, SF, CF, S> {
        self.adapter_builder
            .adapters
            .insert(0, FactoryWrapper::wrap(SocksNoUdpForwardUnpacker));

        self.adapter_builder
    }

    pub fn with_udp_forward<UF, U>(
        mut self,
        udp_forward: UF,
    ) -> PenetrateAdapterBuilder<E, SF, CF, S>
    where
        UF: Factory<(), Output = BoxedFuture<(SocketAddr, U)>> + Send + Sync + 'static,
        U: UdpSocket + Unpin + Send + Sync + 'static,
    {
        let udp_forward = FactoryWrapper::wrap(udp_forward);

        self.adapter_builder.adapters.insert(
            0,
            FactoryWrapper::wrap(SocksUnpacker {
                udp_factory: Arc::new(FactoryWrapper::wrap(udp_forward)),
            }),
        );
        self.adapter_builder
    }
}

impl<S> Factory<Fallback<S>> for SocksNoUdpForwardUnpacker
where
    S: Stream + Send + Sync + 'static,
{
    type Output = BoxedFuture<Adapter<S>>;

    fn call(&self, stream: Fallback<S>) -> Self::Output {
        Box::pin(async move {
            let mut stream = stream;

            log::debug!("call socks5");

            let socket = match stream
                .socks5_handshake(&mut NoAuthentication::default())
                .await
            {
                Err(e) if !e.is_socks_error() => return Err(e),
                Err(_) => return Ok(Adapter::Reject(stream)),
                Ok(socket) => socket,
            };

            stream.force_clear();

            match socket {
                Socket::Tcp(addr) => Ok(Adapter::Accept(Peer::Visitor(
                    Visitor::Forward(stream),
                    Socket::Tcp(addr),
                ))),
                Socket::Udp(_) => Ok({
                    socks::finish_udp_forward(&mut stream).await?;
                    Adapter::Accept(Peer::Finished(stream))
                }),
                _ => unsafe { std::hint::unreachable_unchecked() },
            }
        })
    }
}

impl<S, U> Factory<Fallback<S>> for SocksUnpacker<U>
where
    S: Stream + Send + Sync + 'static,
    U: UdpSocket + Unpin + Send + 'static,
{
    type Output = BoxedFuture<Adapter<S>>;

    fn call(&self, stream: Fallback<S>) -> Self::Output {
        let udp_factory = self.udp_factory.clone();
        Box::pin(async move {
            let mut stream = stream;

            log::debug!("call socks5");

            let socket = match stream
                .socks5_handshake(&mut NoAuthentication::default())
                .await
            {
                Err(e) if !e.is_socks_error() => return Err(e),
                Err(_) => return Ok(Adapter::Reject(stream)),
                Ok(socket) => socket,
            };

            stream.force_clear();

            match socket {
                Socket::Tcp(addr) => Ok(Adapter::Accept(Peer::Visitor(
                    Visitor::Forward(stream),
                    Socket::Tcp(addr),
                ))),
                Socket::Udp(addr) => Ok({
                    log::debug!("addr={}", addr);
                    let stream = stream.into_inner();
                    let udp_forward = SocksUdpForward {
                        udp_factory,
                        guard: std::sync::Mutex::new(Some(stream)),
                    };
                    Adapter::Accept(Peer::Visitor(
                        Visitor::Consume(FactoryWrapper::wrap(udp_forward)),
                        Socket::UdpForward(addr),
                    ))
                }),
                _ => unsafe { std::hint::unreachable_unchecked() },
            }
        })
    }
}

impl<S, U> Factory<Fallback<S>> for SocksUdpForward<S, U>
where
    S: Stream + Send + 'static,
    U: UdpSocket + Unpin + Send + 'static,
{
    type Output = BoxedFuture<()>;

    fn call(&self, s2: Fallback<S>) -> Self::Output {
        let s2 = s2.into_inner();
        let s1 = match self.guard.lock() {
            Err(_) => return Box::pin(async move { Err(Kind::Once.into()) }),
            Ok(mut lock) => match lock.take() {
                None => return Box::pin(async move { Err(Kind::Once.into()) }),
                Some(s) => s,
            },
        };

        let factory = self.udp_factory.clone();

        let fut = async move {
            let mut s1 = s1;
            let mut s2 = s2;

            let (addr, mut udp) = factory.call(()).await?;

            log::debug!("udp forwarding service listening on {}", addr);

            socks::send_udp_forward_message(&mut s1, addr).await?;

            log::debug!("forward message sent");

            let fut1 = async move {
                loop {
                    let mut buf = [1];
                    match s1.read(&mut buf).await {
                        Ok(n) if n == 0 => {
                            log::warn!("udp forwarding aborted");
                            break Err(Into::<std::io::Error>::into(
                                std::io::ErrorKind::ConnectionReset,
                            )
                            .into());
                        }
                        Err(e) => {
                            log::warn!("udp forwarding aborted err={}", e);
                            break Err(e);
                        }
                        _ => {}
                    };
                }
            };

            let fut2 = async move {
                let mut buf = Vec::with_capacity(1500);

                unsafe {
                    buf.set_len(1500);
                }

                log::debug!("waiting for visitor data");

                let (n, addr) = udp.recv_from(&mut buf).await?;

                log::debug!("visitor data received addr={}, size={}bytes", addr, n);

                socks::parse_and_forward_data(&mut s2, &buf[..n]).await?;

                let packet = s2.recv_packet().await?;

                socks::send_packed_udp_forward_message(
                    &mut udp,
                    &addr,
                    &([0, 0, 0, 0], 0).into(),
                    &packet.payload,
                )
                .await?;

                s2.close().await?;

                Ok(())
            };

            Select::select(fut1, fut2).await
        };

        Box::pin(async move {
            match fut.await {
                Ok(()) => {
                    log::debug!("udp forwarding ends");
                    Ok(())
                }
                Err(e) => {
                    log::warn!("udp forwarding error {}", e);
                    Err(e)
                }
            }
        })
    }
}

impl<S, U> Factory<S> for SocksClientUdpForward<U>
where
    S: Stream + Send + 'static,
    U: UdpSocket + Send + Unpin + 'static,
{
    type Output = BoxedFuture<()>;

    fn call(&self, mut stream: S) -> Self::Output {
        let factory = self.0.clone();
        Box::pin(async move {
            let message = stream.recv_packet().await?.try_message()?;

            let addr = match message {
                Message::Forward(addr) => addr,
                message => {
                    log::warn!("wrong message {}", message);
                    return Ok(());
                }
            };

            let (_, mut udp) = factory.call(addr).await?;

            let data = stream.recv_packet().await?;

            let _ = udp.send(&data.payload).await?;

            let mut buf = Vec::with_capacity(1500);

            unsafe {
                buf.set_len(1500);
            }

            let n = udp.recv(&mut buf).await?;
            buf.truncate(n);

            let packet = make_packet(buf).encode();

            stream.send_packet(&packet).await?;

            stream.close().await
        })
    }
}
