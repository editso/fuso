mod direct;

mod socks;

use std::pin::Pin;

use self::socks::PenetrateSocksBuilder;

pub use socks::SocksUdpForwardConverter;

use super::{server::Peer, PenetrateAdapterBuilder};
use crate::{guard::Fallback, Accepter, Executor, Provider, ProviderWrapper, Socket, Stream};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;
pub type Converter<S> = ProviderWrapper<Fallback<S>, Peer<Fallback<S>>>;

impl<E, SP, A, S> PenetrateAdapterBuilder<E, SP, S>
where
    E: Executor + 'static,
    SP: Provider<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
    A: Accepter<Stream = S> + Unpin + Send + 'static,
    S: Stream + Send + Sync + 'static,
{
    pub fn using_direct(mut self) -> Self {
        self.adapters
            .push(ProviderWrapper::wrap(direct::NormalUnpacker));
        self
    }

    pub fn using_socks(self) -> PenetrateSocksBuilder<E, SP, S> {
        PenetrateSocksBuilder {
            adapter_builder: self,
        }
    }
}
