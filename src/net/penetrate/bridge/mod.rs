use std::{net::IpAddr, pin::Pin, sync::Arc, time::Duration};

use crate::{
    client::Client,
    generator::Generator,
    io,
    protocol::{AsyncRecvPacket, AsyncSendPacket, Bind, Poto, ToBytes, TryToPoto},
    select::Select,
    server::Handshake,
    time, Accepter, AccepterExt, Address, ClientProvider, DecorateProvider, Executor, Fuso, Kind,
    Processor, Provider, Serve, Socket, Stream, WrappedProvider,
};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub struct Bridge<E, H, P, S, SP> {
    socket: Socket,
    client: Client<E, H, P, S>,
    bridge_handshake: Option<Arc<Handshake<S>>>,
    accepter_provider: SP,
}

impl<E, H, P, S> Fuso<Client<E, H, P, S>> {
    pub fn using_bridge<A: Into<Socket>, SP, HF>(
        self,
        socket: A,
        provider: SP,
        handshake: HF,
    ) -> Bridge<E, H, P, S, SP>
    where
        HF: Provider<S, Output = BoxedFuture<(S, Option<DecorateProvider<S>>)>>
            + Send
            + Sync
            + 'static,
    {
        Bridge {
            socket: socket.into(),
            client: self.0,
            bridge_handshake: Some(Arc::new(WrappedProvider::wrap(handshake))),
            accepter_provider: provider,
        }
    }
}

impl<E, H, P, S, A, G, SP> Bridge<E, H, P, S, SP>
where
    E: Executor + Send + Sync + 'static,
    P: Provider<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    SP: Provider<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
    A: Accepter<Stream = S> + Send + Sync + Unpin + 'static,
    S: Stream + Send + Sync + 'static,
    G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
    H: Provider<(S, Processor<ClientProvider<P>, S, ()>), Output = BoxedFuture<G>>
        + Send
        + Sync
        + 'static,
{
    async fn run_bridge(
        client: S,
        accepter: Arc<SP>,
        handshake: Option<WrappedProvider<S, (S, Option<DecorateProvider<S>>)>>,
        client_provider: ClientProvider<P>,
        upstream: Socket,
        executor: Arc<E>,
        bridge_handshake: Option<Arc<Handshake<S>>>,
    ) -> crate::Result<()> {
        let (mut client, client_decorate) = match bridge_handshake.as_ref() {
            Some(handshake) => handshake.call(client).await?,
            None => (client, None),
        };

        let poto = client.recv_packet().await?.try_poto()?;

        let (upstream, client_addr, proxy_accepter, upstream_decorate) = match poto {
            Poto::Bind(Bind::Setup(client_addr, visitor_addr)) => {
                let upstream_addr = upstream;
                let upstream = client_provider.call(upstream_addr.clone()).await?;

                let (mut upstream, decorate) = match handshake {
                    Some(handshake) => handshake.call(upstream).await?,
                    None => (upstream, None),
                };

                let poto =
                    Poto::Bind(Bind::Setup(client_addr.clone(), visitor_addr.clone())).bytes();

                upstream.send_packet(&poto).await?;

                let poto = upstream.recv_packet().await?.try_poto()?;

                match poto {
                    Poto::Bind(Bind::Success(mut client_accepter, visitor_accepter)) => {
                        let proxy_accepter = accepter.call(client_addr).await?;
                        let poto = Poto::Bind(Bind::Success(
                            proxy_accepter.local_addr()?,
                            visitor_accepter,
                        ))
                        .bytes();

                        client.send_packet(&poto).await?;

                        if client_accepter.is_ip_unspecified() {
                            client_accepter.from_set_host(upstream_addr.addr());
                        }

                        if client_accepter.is_ip_unspecified() {
                            client_accepter.set_ip([127, 0, 0, 1]);
                        }

                        log::info!(
                            "bridge from {} to {} -> {}",
                            client.peer_addr()?,
                            proxy_accepter.local_addr()?,
                            client_accepter
                        );

                        (upstream, client_accepter, proxy_accepter, decorate)
                    }
                    _ => {
                        let poto = poto.bytes();
                        return client.send_packet(&poto).await;
                    }
                }
            }
            poto => {
                let poto = poto.bytes();
                return client.send_packet(&poto).await;
            }
        };

        let f1 = io::forward(client, upstream);

        let f2 = async move {
            let mut proxy_accepter = proxy_accepter;
            let client_decorate = client_decorate;
            let upstream_decorate = upstream_decorate;

            loop {
                let stream = proxy_accepter.accept().await?;

                let socket = match stream.peer_addr()? {
                    Address::One(socket) => socket,
                    Address::Many(_) => unsafe { std::hint::unreachable_unchecked() },
                };

                let upstream = match client_addr.select(&socket) {
                    Ok(socket) => client_provider.call(socket),
                    Err(e) => {
                        log::warn!("failed to bridge: {}", e);
                        continue;
                    }
                };

                let client_decorate = client_decorate.clone();
                let upstream_decorate = upstream_decorate.clone();

                let forward_fn = move || async move {
                    let stream = client_decorate.call(stream).await?;

                    let upstream = upstream_decorate.call(upstream.await?).await?;

                    log::debug!(
                        "bridge {} -> {}",
                        stream.peer_addr()?,
                        upstream.peer_addr()?
                    );

                    io::forward(stream, upstream).await
                };

                executor.spawn(async move {
                    let result = match time::wait_for(Duration::from_secs(10), forward_fn()).await {
                        Ok(Err(e)) => Err(e),
                        Err(e) => Err(e),
                        _ => Ok(()),
                    };

                    if let Err(e) = result {
                        log::debug!("failed to bridge {}", e);
                    }
                });
            }
        };

        Select::select(f1, f2).await
    }

    async fn run_async(self) -> crate::Result<()> {
        let bridge_socket = self.socket;
        let accepter_provider = Arc::new(self.accepter_provider);
        let executor = self.client.executor.clone();
        let server_socket = self.client.socket.clone();
        let handshake = self.client.handshake.clone();
        let client_provider = self.client.client_provider.clone();
        let bridge_handshake = self.bridge_handshake.clone();

        if bridge_socket.eq(&server_socket)
            || (bridge_socket.is_ip_unspecified()
                && server_socket.ip().eq(&Some(IpAddr::from([127, 0, 0, 1]))))
        {
            return Err(Kind::AddressLoop(bridge_socket).into());
        }

        let bridge = async move {
            let mut accepter = accepter_provider.call(bridge_socket).await?;

            log::info!("bridge listening on {}", accepter.local_addr()?);

            loop {
                let client = accepter.accept().await?;
                let handshake = handshake.clone();
                executor.spawn(Self::run_bridge(
                    client,
                    accepter_provider.clone(),
                    handshake,
                    client_provider.clone(),
                    server_socket.clone(),
                    executor.clone(),
                    bridge_handshake.clone(),
                ));
            }
        };

        let client = Fuso(self.client);

        Select::select(client.run(), bridge).await
    }

    pub fn run(self) -> Fuso<Serve> {
        Fuso(Serve {
            fut: Box::pin(self.run_async()),
        })
    }
}
