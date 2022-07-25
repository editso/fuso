use std::{pin::Pin, sync::Arc, time::Duration};

use crate::{
    client::{Client, ClientBuilder, Route},
    guard::Fallback,
    server::{Server, ServerBuilder},
    Accepter, Executor, Provider, ProviderWrapper, Fuso, Socket, Stream,
};

use super::{
    client::PenetrateClientProvider,
    server::{Config, Peer, PenetrateProvider},
};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub struct PenetrateServerBuilder<E, SF, CF, S> {
    is_mixed: bool,
    max_wait_time: Duration,
    heartbeat_timeout: Duration,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    fallback_strict_mode: bool,
    server_builder: ServerBuilder<E, SF, CF, S>,
}

pub struct PenetrateClientBuilder<E, CF, S> {
    /// 上游地址，也就是服务端地址
    upstream: Socket,
    /// 下游地址， 也就是本地需要映射的地址
    downstream: Socket,
    /// 创建连接等待时间, 超过视为超时
    maximum_wait: Option<Duration>,
    /// 重连延时
    reconnect_delay: Option<Duration>,
    /// 重连尝试次数，如果为None那么永不停止
    maximum_retries: Option<usize>,
    /// 心跳延时
    heartbeat_delay: Option<Duration>,
    client_builder: ClientBuilder<E, CF, S>,
}

impl<E, SF, CF, S> ServerBuilder<E, SF, CF, S> {
    pub fn with_penetrate(self) -> PenetrateServerBuilder<E, SF, CF, S> {
        PenetrateServerBuilder {
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

impl<E, SF, CF, A, S> PenetrateServerBuilder<E, SF, CF, S>
where
    E: Executor + 'static,
    SF: Provider<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
    CF: Provider<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
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

    pub fn build<F>(self, unpacker: F) -> Fuso<Server<E, PenetrateProvider<S>, SF, CF, S>>
    where
        F: Provider<Fallback<S>, Output = BoxedFuture<Peer<Fallback<S>>>> + Send + Sync + 'static,
    {
        self.server_builder.build(PenetrateProvider {
            config: Config {
                is_mixed: self.is_mixed,
                max_wait_time: self.max_wait_time,
                heartbeat_timeout: self.heartbeat_timeout,
                read_timeout: self.read_timeout,
                write_timeout: self.write_timeout,
                fallback_strict_mode: self.fallback_strict_mode,
            },
            unpacker: Arc::new(ProviderWrapper::wrap(unpacker)),
        })
    }
}

impl<E, CF, S> ClientBuilder<E, CF, S> {
    pub fn using_penetrate<U: Into<Socket>>(
        self,
        upstream: U,
        downstream: U,
    ) -> PenetrateClientBuilder<E, CF, S> {
        PenetrateClientBuilder {
            upstream: upstream.into(),
            downstream: downstream.into(),
            client_builder: self,
            maximum_wait: None,
            maximum_retries: None,
            reconnect_delay: None,
            heartbeat_delay: None,
        }
    }
}

impl<E, CF, S> PenetrateClientBuilder<E, CF, S>
where
    E: Executor + 'static,
    CF: Provider<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    S: Stream + Send + 'static,
{
    pub fn reconnect_delay(mut self, delay: Duration) -> Self {
        self.reconnect_delay = Some(delay);
        self
    }

    pub fn maximum_retries(mut self, maximum_retries: Option<usize>) -> Self {
        self.maximum_retries = maximum_retries;
        self
    }

    pub fn heartbeat_delay(mut self, delay: Duration) -> Self {
        self.heartbeat_delay = Some(delay);
        self
    }

    pub fn maximum_wait(mut self, time: Duration) -> Self {
        self.maximum_wait = Some(time);
        self
    }

    pub fn build<C>(self, connector: C) -> Fuso<Client<E, PenetrateClientProvider<C>, CF, S>>
    where
        C: Provider<Socket, Output = BoxedFuture<Route<S>>> + Unpin + Send + Sync + 'static,
    {
        self.client_builder.build(
            self.upstream.clone(),
            PenetrateClientProvider {
                transform: (self.upstream, self.downstream),
                connector_provider: Arc::new(connector),
            },
        )
    }
}
