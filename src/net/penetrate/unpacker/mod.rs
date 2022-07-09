mod normal;

mod socks;

use super::{server::Peer, PenetrateAdapterBuilder};
use crate::{FactoryWrapper, guard::Fallback, Stream};

pub type Unpacker<S> = FactoryWrapper<Fallback<S>, Peer<Fallback<S>>>;

impl<E, SF, CF, S> PenetrateAdapterBuilder<E, SF, CF, S>
where
    S: Stream + Send + 'static,
{
    pub fn use_normal(mut self) -> Self {
        self.adapters
            .push(FactoryWrapper::wrap(normal::NormalUnpacker));
        self
    }

    pub fn use_socks(mut self) -> Self {
        
        self
    }
}
