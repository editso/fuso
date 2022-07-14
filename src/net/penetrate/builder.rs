use std::{pin::Pin, sync::Arc, time::Duration};

use crate::{
    guard::Fallback,
    server::{Server, ServerBuilder},
    Accepter, Executor, Factory, FactoryWrapper, Fuso, Socket, Stream,
};

use super::server::{Config, Peer, PenetrateFactory};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub struct PenetrateBuilder<E, SF, CF, S> {
    is_mixed: bool,
    max_wait_time: Duration,
    heartbeat_timeout: Duration,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    fallback_strict_mode: bool,
    server_builder: ServerBuilder<E, SF, CF, S>,
}

impl<E, SF, CF, S> ServerBuilder<E, SF, CF, S> {
    pub fn with_penetrate(self) -> PenetrateBuilder<E, SF, CF, S> {
        PenetrateBuilder {
            is_mixed: self.is_mixed,
            write_timeout: None,
            read_timeout: None,
            max_wait_time: Duration::from_secs(10),
            heartbeat_timeout: Duration::from_secs(60),
            fallback_strict_mode: true,
            server_builder: self,
        }
    }
}

impl<E, SF, CF, A, S> PenetrateBuilder<E, SF, CF, S>
where
    E: Executor + 'static,
    SF: Factory<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
    CF: Factory<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
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

    pub fn enable_fallback_strict_mode(mut self) -> Self {
        self.fallback_strict_mode = true;
        self
    }

    pub fn disable_fallback_strict_mode(mut self) -> Self {
        self.fallback_strict_mode = false;
        self
    }

    pub fn build<F>(self, unpacker: F) -> Fuso<Server<E, PenetrateFactory<S>, SF, CF, S>>
    where
        F: Factory<Fallback<S>, Output = BoxedFuture<Peer<Fallback<S>>>> + Send + Sync + 'static,
    {
        self.server_builder.build(PenetrateFactory {
            config: Config {
                is_mixed: self.is_mixed,
                max_wait_time: self.max_wait_time,
                heartbeat_timeout: self.heartbeat_timeout,
                read_timeout: self.read_timeout,
                write_timeout: self.write_timeout,
                fallback_strict_mode: self.fallback_strict_mode,
            },
            unpacker: Arc::new(FactoryWrapper::wrap(unpacker)),
        })
    }
}
