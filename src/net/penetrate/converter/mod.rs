mod direct;

mod socks;

use std::pin::Pin;

use self::socks::PenetrateSocksBuilder;

pub use socks::SocksUdpForwardConverter;

use super::{server::Peer, PenetrateAdapterBuilder};
use crate::{guard::Fallback, Accepter, Executor, Provider, Socket, Stream, WrappedProvider};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;
pub type Converter<S> = WrappedProvider<Fallback<S>, Peer<Fallback<S>>>;

impl<E, P, A, S, O> PenetrateAdapterBuilder<E, P, S, O>
where
    E: Executor + 'static,
    A: Accepter<Stream = S> + Unpin + Send + 'static,
    S: Stream + Send + Sync + 'static,
    P: Provider<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
{
    pub fn using_direct(mut self) -> Self {
        self.adapters
            .push(WrappedProvider::wrap(direct::DirectConverter));
        self
    }

    pub fn using_socks(self) -> PenetrateSocksBuilder<E, P, S, O> {
        PenetrateSocksBuilder {
            adapter_builder: self,
        }
    }
}
