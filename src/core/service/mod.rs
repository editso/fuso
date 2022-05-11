use crate::{connector::Connector, listener::Accepter};

pub trait Service {
    type Config;
    type Socket;
    type Stream;
    type Accepter: Accepter<Stream = Self::Stream>;
    type Connector: Connector<Stream = Self::Stream, Socket = Self::Socket>;
    type Future: std::future::Future<Output = crate::Result<(Self::Accepter, Self::Connector)>>;

    fn create(&self, config: &Self::Config) -> Self::Future;
}


