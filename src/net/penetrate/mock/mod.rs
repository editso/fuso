mod direct;

mod socks;

use std::{pin::Pin, sync::Arc};

use self::socks::PenetrateSocksBuilder;

pub use socks::SocksUdpForwardMock;

use super::{server::Peer, PenetrateSelectorBuilder};
use crate::{guard::Fallback, Accepter, Executor, Provider, Socket, Stream, WrappedProvider};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub type Mock<S> = WrappedProvider<(Fallback<S>, Arc<super::server::Config>), Peer<Fallback<S>>>;

impl<E, P, A, S, O> PenetrateSelectorBuilder<E, P, S, O>
where
    E: Executor + 'static,
    A: Accepter<Stream = S> + Unpin + Send + 'static,
    S: Stream + Send + Sync + 'static,
    P: Provider<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
{
    pub fn using_direct(mut self) -> Self {
        self.adapters
            .push(WrappedProvider::wrap(direct::DirectMock));
        self
    }

    pub fn using_socks(self) -> PenetrateSocksBuilder<E, P, S, O> {
        PenetrateSocksBuilder {
            adapter_builder: self,
        }
    }
}
