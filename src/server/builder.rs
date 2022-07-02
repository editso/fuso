use std::{pin::Pin, sync::Arc, time::Duration};

use crate::{
    generator::Generator,
    guard::Fallback,
    listener::Accepter,
    middleware::{FactoryChain, FactoryTransfer},
    service::{Factory, ServerFactory},
    Addr, Executor, Fuso, Stream,
};

use super::{
    penetrate::{Config, Peer, PenetrateFactory},
    Server,
};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub struct PenetrateBuilder<E, SF, CF, S> {
    max_wait_time: Duration,
    heartbeat_timeout: Duration,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    server_builder: ServerBuilder<E, SF, CF, S>,
}

pub struct ServerBuilder<E, SF, CF, S> {
    pub(crate) executor: E,
    pub(crate) handshake: Option<FactoryTransfer<S>>,
    pub(crate) server_factory: ServerFactory<SF, CF>,
}

impl<E, SF, CF, S> ServerBuilder<E, SF, CF, S>
where
    S: Stream + Send + 'static,
{
    pub fn with_handshake<F>(mut self, handshake: F) -> Self
    where
        F: Factory<S, Output = BoxedFuture<S>> + Send + Sync + 'static,
    {
        match self.handshake.take() {
            None => {
                self.handshake = Some(FactoryTransfer::wrap(handshake));
            }
            Some(wrapper) => {
                self.handshake = Some(FactoryTransfer::wrap(FactoryChain::chain(
                    wrapper, handshake,
                )));
            }
        }
        self
    }

    pub fn build<H, G>(self, handler: H) -> Fuso<Server<E, H, SF, CF, S>>
    where
        H: Factory<(ServerFactory<SF, CF>, S), Output = BoxedFuture<G>> + Send + Sync + 'static,
        G: Generator<Output = Option<BoxedFuture<()>>> + Send + 'static,
    {
        Fuso(Server {
            handler: Arc::new(handler),
            bind: ([0, 0, 0, 0], 0).into(),
            executor: self.executor,
            factory: self.server_factory,
            handshake: self.handshake.map(Arc::new),
        })
    }
}

impl<E, SF, CF, S> ServerBuilder<E, SF, CF, S> {
    pub fn with_penetrate(self) -> PenetrateBuilder<E, SF, CF, S> {
        PenetrateBuilder {
            write_timeout: None,
            read_timeout: None,
            max_wait_time: Duration::from_secs(10),
            heartbeat_timeout: Duration::from_secs(60),
            server_builder: self,
        }
    }
}

impl<E, SF, CF, A, S> PenetrateBuilder<E, SF, CF, S>
where
    E: Executor + 'static,
    SF: Factory<Addr, Output = BoxedFuture<A>> + Send + Sync + 'static,
    CF: Factory<Addr, Output = BoxedFuture<S>> + Send + Sync + 'static,
    A: Accepter<Stream = S> + Unpin + Send + 'static,
    S: Stream + Send + Sync + 'static,
{
    pub fn read_timeout(mut self, time: Option<Duration>) -> Self {
        self.read_timeout = time;
        self
    }

    pub fn write_timeout(mut self, time: Option<Duration>) -> Self {
        self.write_timeout = time;
        self
    }

    pub fn max_wait_time(mut self, time: Duration) -> Self {
        self.max_wait_time = time.min(Duration::from_secs(10));
        self
    }

    pub fn heartbeat_timeout(mut self, time: Duration) -> Self {
        self.heartbeat_timeout = time.min(Duration::from_secs(60));
        self
    }

    pub fn build<F>(self, factory: F) -> Fuso<Server<E, PenetrateFactory<S>, SF, CF, S>>
    where
        F: Factory<Peer<Fallback<S>>, Output = BoxedFuture<Peer<Fallback<S>>>>
            + Send
            + Sync
            + 'static,
    {
        self.server_builder.build(PenetrateFactory {
            config: Config {
                max_wait_time: self.max_wait_time,
                heartbeat_timeout: self.heartbeat_timeout,
                read_timeout: self.read_timeout,
                write_timeout: self.write_timeout,
            },
            factory: Arc::new(FactoryTransfer::wrap(factory)),
        })
    }
}
