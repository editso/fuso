mod normal;

mod socks;

use std::pin::Pin;

use super::{server::Peer, PenetrateAdapterBuilder};
use crate::{guard::Fallback, Accepter, Executor, Factory, FactoryWrapper, Socket, Stream};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;
pub type Unpacker<S> = FactoryWrapper<Fallback<S>, Peer<Fallback<S>>>;

impl<E, SF, CF, A, S> PenetrateAdapterBuilder<E, SF, CF, S>
where
    E: Executor + 'static,
    SF: Factory<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
    CF: Factory<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    A: Accepter<Stream = S> + Unpin + Send + 'static,
    S: Stream + Send + Sync + 'static,
{
    pub fn use_normal(mut self) -> Self {
        self.adapters
            .push(FactoryWrapper::wrap(normal::NormalUnpacker));
        self
    }

    pub fn use_socks(mut self) -> Self {
        self.penetrate_builder = self.penetrate_builder.disable_fallback_strict_mode();

        self.adapters
            .insert(0, FactoryWrapper::wrap(socks::SocksUnpacker));
        self
    }
}
