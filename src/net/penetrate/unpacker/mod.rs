mod normal;

mod socks;

use std::pin::Pin;

use self::socks::PenetrateSocksBuilder;

pub use socks::SocksClientUdpForward;

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
    pub fn with_normal_unpacker(mut self) -> Self {
        self.adapters
            .push(FactoryWrapper::wrap(normal::NormalUnpacker));
        self
    }

    pub fn with_socks_unpacker(self) -> PenetrateSocksBuilder<E, SF, CF, S> {
        PenetrateSocksBuilder {
            adapter_builder: self
        }
    }
}
