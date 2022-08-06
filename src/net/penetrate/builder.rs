use std::{pin::Pin, sync::Arc, time::Duration};

use crate::{
    client::{Client, ClientBuilder, Route},
    guard::Fallback,
    server::{Server, ServerBuilder},
    Accepter, Executor, Fuso, Provider, Socket, Stream, WrappedProvider, Platform,
};

use super::{
    client::PenetrateClientProvider,
    server::{Config, Peer, PenetrateProvider},
    PenetrateObserver,
};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub struct PenetrateServerBuilder<E, P, S, O> {
    is_mixed: bool,
    max_wait_time: Duration,
    heartbeat_timeout: Duration,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    fallback_strict_mode: bool,
    server_builder: ServerBuilder<E, P, S, O>,
}

pub struct PenetrateClientBuilder<E, CF, S> {
    /// 服务名
    name: String,
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
    /// 是否启用 kcp
    enable_kcp: bool,
    /// 是否启用socks5
    enable_socks5: bool,
    /// socks5用户名
    socks_username: Option<String>,
    /// socks5密码
    socks_password: Option<String>,
    /// 是否启用socks5 udp转发
    enable_socks5_udp: bool,
    /// builder ...
    client_builder: ClientBuilder<E, CF, S>,
}

impl<E, P, S, O> ServerBuilder<E, P, S, O> {
    pub fn using_penetrate(self) -> PenetrateServerBuilder<E, P, S, O> {
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

impl<E, P, A, S, O> PenetrateServerBuilder<E, P, S, O>
where
    E: Executor + 'static,
    A: Accepter<Stream = S> + Unpin + Send + 'static,
    S: Stream + Send + Sync + 'static,
    P: Provider<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
    O: PenetrateObserver + Send + Sync + 'static,
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

    pub fn build<F>(self, mock: F) -> Fuso<Server<E, PenetrateProvider<S>, P, S, O>>
    where
        F: Provider<
                (Fallback<S>, Arc<super::server::Config>),
                Output = BoxedFuture<Peer<Fallback<S>>>,
            > + Send
            + Sync
            + 'static,
    {
        self.server_builder.build(PenetrateProvider {
            config: Config {
                whoami: String::from("anonymous"),
                is_mixed: self.is_mixed,
                maximum_wait: self.max_wait_time,
                heartbeat_delay: self.heartbeat_timeout,
                read_timeout: self.read_timeout,
                write_timeout: self.write_timeout,
                fallback_strict_mode: self.fallback_strict_mode,
                enable_socks: false,
                enable_socks_udp: false,
                socks5_password: None,
                socks5_username: None,
                platform: Default::default()
            },
            mock: Arc::new(WrappedProvider::wrap(mock)),
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
            name: String::from("anonymous"),
            upstream: upstream.into(),
            downstream: downstream.into(),
            client_builder: self,
            maximum_wait: None,
            maximum_retries: None,
            reconnect_delay: None,
            heartbeat_delay: None,
            enable_kcp: false,
            enable_socks5: false,
            socks_username: None,
            socks_password: None,
            enable_socks5_udp: false,
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
        self.reconnect_delay = Some(delay.min(Duration::from_secs(2)));
        self
    }

    pub fn set_name(mut self, name: String) -> Self {
        self.name = name;
        self
    }

    pub fn enable_kcp(mut self, enable: bool) -> Self {
        self.enable_kcp = enable;
        self
    }

    pub fn enable_socks5(mut self, enable: bool) -> Self {
        self.enable_socks5 = enable;
        self
    }

    pub fn enable_socks5_udp(mut self, enable: bool) -> Self {
        self.enable_socks5_udp = enable;
        self
    }

    pub fn set_socks5_username(mut self, username: Option<String>) -> Self {
        self.socks_username = username;
        self
    }

    pub fn set_socks5_password(mut self, password: Option<String>) -> Self {
        self.socks_password = password;
        self
    }

    pub fn maximum_retries(mut self, maximum_retries: Option<usize>) -> Self {
        self.maximum_retries = maximum_retries;
        self
    }

    pub fn heartbeat_delay(mut self, delay: Duration) -> Self {
        self.heartbeat_delay = Some(delay.min(Duration::from_secs(30)));
        self
    }

    pub fn maximum_wait(mut self, time: Duration) -> Self {
        self.maximum_wait = Some(time.min(Duration::from_secs(10)));
        self
    }

    pub fn build<A: Into<Socket>, C>(
        self,
        server_socket: A,
        connector: C,
    ) -> Fuso<Client<E, PenetrateClientProvider<C>, CF, S>>
    where
        C: Provider<Socket, Output = BoxedFuture<Route<S>>> + Unpin + Send + Sync + 'static,
    {
        ClientBuilder {
            executor: self.client_builder.executor,
            retry_delay: self.reconnect_delay,
            maximum_retries: self.maximum_retries,
            handshake: self.client_builder.handshake,
            client_provider: self.client_builder.client_provider,
        }
        .build(
            server_socket,
            PenetrateClientProvider {
                forward: (self.upstream, self.downstream),
                connector_provider: Arc::new(connector),
                config: super::client::Config {
                    name: self.name,
                    maximum_wait: self.maximum_wait.unwrap_or(Duration::from_secs(10)),
                    heartbeat_delay: self.heartbeat_delay.unwrap_or(Duration::from_secs(30)),
                    enable_kcp: self.enable_kcp,
                    enable_socks5: self.enable_socks5,
                    socks_username: self.socks_username,
                    socks_password: self.socks_password,
                    enable_socks5_udp: self.enable_socks5_udp,
                    version: String::from(env!("CARGO_PKG_VERSION")),
                    platform: Platform::default()
                },
            },
        )
    }
}
