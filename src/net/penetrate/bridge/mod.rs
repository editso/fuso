use std::{net::IpAddr, pin::Pin};

use crate::{
    client::Client, generator::Generator, io, select::Select, Accepter, AccepterExt,
    ClientProvider, Executor, Fuso, Kind, Processor, Provider, Serve, Socket, Stream,
};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub struct Bridge<E, H, P, S, SP> {
    socket: Socket,
    client: Client<E, H, P, S>,
    accepter_provider: SP,
}

impl<E, H, P, S> Fuso<Client<E, H, P, S>> {
    pub fn using_bridge<A: Into<Socket>, SP>(
        self,
        socket: A,
        provider: SP,
    ) -> Bridge<E, H, P, S, SP> {
        Bridge {
            socket: socket.into(),
            client: self.0,
            accepter_provider: provider,
        }
    }
}

impl<E, H, P, S, A, G, SP> Bridge<E, H, P, S, SP>
where
    E: Executor + 'static,
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
    async fn run_async(self) -> crate::Result<()> {
        let bridge_socket = self.socket;
        let accepter = self.accepter_provider;
        let executor = self.client.executor.clone();
        let server_socket = self.client.socket.clone();
        let client_provider = self.client.client_provider.clone();

        if bridge_socket.eq(&server_socket)
            || (bridge_socket.is_ip_unspecified()
                && server_socket.ip().eq(&Some(IpAddr::from([127, 0, 0, 1]))))
        {
            return Err(Kind::AddressLoop(bridge_socket).into());
        }

        let bridge = async move {
            let mut accepter = accepter.call(bridge_socket).await?;

            log::info!("bridge listening on {}", accepter.local_addr()?);

            loop {
                let client = accepter.accept().await?;
                let bridge_fut = client_provider.connect(server_socket.clone());
                executor.spawn(async move {
                    match bridge_fut.await {
                        Ok(stream) => {
                            if let Err(e) = io::forward(client, stream).await {
                                log::warn!("bridge error {}", e);
                            }
                        }
                        Err(e) => {
                            log::warn!("bridge error {}", e);
                        }
                    };
                });
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
