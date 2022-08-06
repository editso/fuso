use std::{net::SocketAddr, pin::Pin, sync::Arc};

use crate::{
    ext::AsyncReadExt,
    guard::Fallback,
    io,
    penetrate::{
        server::{Peer, Visitor},
        Adapter, PenetrateAdapterBuilder,
    },
    protocol::{make_packet, AsyncRecvPacket, AsyncSendPacket, Poto, ToBytes, TryToPoto},
    select::Select,
    socks::{self, S5Authenticate, Socks},
    Addr, Kind, Provider, Socket, SocketKind, Stream, UdpReceiverExt, UdpSocket, WrappedProvider,
};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub struct PenetrateSocksBuilder<E, P, S, O> {
    pub(crate) adapter_builder: PenetrateAdapterBuilder<E, P, S, O>,
}

pub struct SocksConverter<U> {
    pub(crate) udp_provider: Arc<WrappedProvider<(), (SocketAddr, U)>>,
}

pub struct SimpleSocksConverter;

pub struct SocksUdpForwardConverter<U>(pub(crate) WrappedProvider<Addr, (SocketAddr, U)>);

pub struct SocksUdpForward<S, U> {
    udp_provider: Arc<WrappedProvider<(), (SocketAddr, U)>>,
    guard: std::sync::Mutex<Option<S>>,
}

impl<E, P, S, O> PenetrateSocksBuilder<E, P, S, O>
where
    S: Stream + Send + Sync + 'static,
{
    pub fn no_udp_forward(mut self) -> PenetrateAdapterBuilder<E, P, S, O> {
        self.adapter_builder
            .adapters
            .insert(0, WrappedProvider::wrap(SimpleSocksConverter));

        self.adapter_builder
    }

    pub fn using_udp_forward<UF, U>(
        mut self,
        udp_forward: UF,
    ) -> PenetrateAdapterBuilder<E, P, S, O>
    where
        UF: Provider<(), Output = BoxedFuture<(SocketAddr, U)>> + Send + Sync + 'static,
        U: UdpSocket + Unpin + Send + Sync + 'static,
    {
        let udp_forward = WrappedProvider::wrap(udp_forward);

        self.adapter_builder.adapters.insert(
            0,
            WrappedProvider::wrap(SocksConverter {
                udp_provider: Arc::new(WrappedProvider::wrap(udp_forward)),
            }),
        );
        self.adapter_builder
    }
}

impl<S> Provider<Fallback<S>> for SimpleSocksConverter
where
    S: Stream + Send + Sync + 'static,
{
    type Output = BoxedFuture<Adapter<S>>;

    fn call(&self, stream: Fallback<S>) -> Self::Output {
        Box::pin(async move {
            let mut stream = stream;

            let socket = match stream
                .socks5_handshake(&mut S5Authenticate::default())
                .await
            {
                Err(e) if !e.is_socks_error() => return Err(e),
                Err(_) => return Ok(Adapter::Reject(stream)),
                Ok(socket) => socket,
            };

            stream.consume_back_data();

            match socket.kind() {
                SocketKind::Tcp => Ok(Adapter::Accept(Peer::Route(Visitor::Route(stream), socket))),
                SocketKind::Udp => Ok({
                    socks::finish_udp_forward(&mut stream).await?;
                    Adapter::Accept(Peer::Finished(stream))
                }),
                _ => unsafe { std::hint::unreachable_unchecked() },
            }
        })
    }
}

impl<S, U> Provider<Fallback<S>> for SocksConverter<U>
where
    S: Stream + Send + Sync + 'static,
    U: UdpSocket + Unpin + Send + Sync + 'static,
{
    type Output = BoxedFuture<Adapter<S>>;

    fn call(&self, stream: Fallback<S>) -> Self::Output {
        let udp_provider = self.udp_provider.clone();
        Box::pin(async move {
            let mut stream = stream;

            let socket = match stream
                .socks5_handshake(&mut S5Authenticate::default())
                .await
            {
                Err(e) if !e.is_socks_error() => return Err(e),
                Err(_) => return Ok(Adapter::Reject(stream)),
                Ok(socket) => socket,
            };

            stream.consume_back_data();

            match socket.kind() {
                SocketKind::Tcp => Ok(Adapter::Accept(Peer::Route(Visitor::Route(stream), socket))),
                SocketKind::Udp => Ok({
                    let stream = stream.into_inner();
                    let udp_forward = SocksUdpForward {
                        udp_provider,
                        guard: std::sync::Mutex::new(Some(stream)),
                    };

                    Adapter::Accept(Peer::Route(
                        Visitor::Provider(WrappedProvider::wrap(udp_forward)),
                        Socket::ufd(socket.into_addr()),
                    ))
                }),
                _ => unsafe { std::hint::unreachable_unchecked() },
            }
        })
    }
}

impl<S, U> Provider<Fallback<S>> for SocksUdpForward<S, U>
where
    S: Stream + Send + 'static,
    U: UdpSocket + Unpin + Send + Sync + 'static,
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

        let provider = self.udp_provider.clone();

        let fut = async move {
            let mut s1 = s1;
            let peer_addr = s2.peer_addr()?;
            let (mut reader, mut writer) = io::split(s2);

            let (addr, mut udp) = provider.call(()).await?;

            log::debug!("udp forwarding service listening on {}", addr);

            socks::send_udp_forward_message(&mut s1, addr).await?;

            let fut1 = {
                let mut writer = writer.clone();
                async move {
                    let mut buf = [1];

                    loop {
                        let r = s1.read(&mut buf).await;

                        if r.is_err() {
                            break;
                        }

                        let n = unsafe { r.unwrap_unchecked() };

                        if n == 0 {
                            break;
                        }
                    }

                    let close = Poto::Close.bytes();

                    writer.send_packet(&close).await
                }
            };

            let fut2 = async move {
                let mut buf = Vec::with_capacity(1500);

                unsafe {
                    buf.set_len(1500);
                }

                loop {
                    let (n, addr) = udp.recv_from(&mut buf).await?;
                    let origin = socks::parse_and_forward_data(&mut writer, &buf[..n]).await?;
                    log::info!("connect from {} to {}", peer_addr, origin);

                    let packet = reader.recv_packet().await?;

                    socks::send_packed_udp_forward_message(
                        &mut udp,
                        &addr,
                        origin,
                        &packet.payload,
                    )
                    .await?;
                }
            };

            Select::select(fut1, fut2).await
        };

        Box::pin(async move {
            match fut.await {
                Ok(()) => {
                    log::debug!("forward packet success");
                    Ok(())
                }
                Err(e) => {
                    log::warn!("failed to forward {}", e);
                    Err(e)
                }
            }
        })
    }
}

impl<S, U> Provider<S> for SocksUdpForwardConverter<U>
where
    S: Stream + Send + 'static,
    U: UdpSocket + Send + Unpin + Sync + 'static,
{
    type Output = BoxedFuture<()>;

    fn call(&self, mut stream: S) -> Self::Output {
        let provider = self.0.clone();
        Box::pin(async move {
            let mut buf = Vec::with_capacity(1500);

            unsafe {
                buf.set_len(1500);
            }

            loop {
                let message = stream.recv_packet().await?.try_poto()?;

                let addr = match message {
                    Poto::Forward(addr) => addr,
                    Poto::Close => {
                        log::debug!("close udp forward");
                        break Ok(());
                    }
                    message => {
                        log::warn!("wrong message {}", message);
                        break Ok(());
                    }
                };

                let (_, udp) = provider.call(addr).await?;

                let data = stream.recv_packet().await?;

                let _ = udp.send(&data.payload).await?;

                log::info!(
                    "connect from {} to {} forward {}bytes",
                    stream.local_addr()?,
                    udp.peer_addr()?,
                    data.payload.len()
                );

                let n = udp.recv(&mut buf).await?;

                let packet = make_packet(buf[..n].to_vec()).encode();

                stream.send_packet(&packet).await?;

                log::debug!("forward success {}bytes", n);
            }
        })
    }
}
