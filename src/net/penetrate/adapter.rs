use std::sync::Arc;

use crate::{
    factory::FactoryWrapper, guard::Fallback, listener::Accepter, server::Server, service::Factory,
    Addr, Executor, Fuso, Stream,
};

use super::{BoxedFuture, Peer, PenetrateBuilder, PenetrateFactory};

pub type UnpackerAdapterFactory<S> = FactoryWrapper<Fallback<S>, UnpackerAdapterOutcome<S>>;

pub struct UnpackerAdapter<S>(Arc<Vec<UnpackerAdapterFactory<S>>>);

pub enum UnpackerAdapterOutcome<O> {
    Accepted(Peer<Fallback<O>>),
    Unacceptable(Fallback<O>),
}

pub struct PenetrateAdapterBuilder<E, SF, CF, S> {
    adapters: Vec<UnpackerAdapterFactory<S>>,
    penetrate_builder: PenetrateBuilder<E, SF, CF, S>,
}

impl<E, SF, CF, S> PenetrateBuilder<E, SF, CF, S> {
    pub fn with_unpacker_adapter_mode(self) -> PenetrateAdapterBuilder<E, SF, CF, S> {
        PenetrateAdapterBuilder {
            adapters: Vec::new(),
            penetrate_builder: self,
        }
    }
}

impl<E, A, SF, CF, S> PenetrateAdapterBuilder<E, SF, CF, S>
where
    E: Executor + 'static,
    SF: Factory<Addr, Output = BoxedFuture<A>> + Send + Sync + 'static,
    CF: Factory<Addr, Output = BoxedFuture<S>> + Send + Sync + 'static,
    A: Accepter<Stream = S> + Unpin + Send + 'static,
    S: Stream + Send + Sync + 'static,
{
    pub fn append_unpacker_adapter<F>(mut self, factory: F) -> Self
    where
        F: Factory<Fallback<S>, Output = BoxedFuture<UnpackerAdapterOutcome<S>>>
            + Send
            + Sync
            + 'static,
    {
        self.adapters.push(FactoryWrapper::wrap(factory));
        self
    }

    pub fn build(self) -> Fuso<Server<E, PenetrateFactory<S>, SF, CF, S>> {
        self.penetrate_builder
            .disable_fallback_strict_mode()
            .build(UnpackerAdapter(Arc::new(self.adapters)))
    }
}

impl<S> Factory<Fallback<S>> for UnpackerAdapter<S>
where
    S: Stream + Send + Unpin + 'static,
{
    type Output = BoxedFuture<Peer<Fallback<S>>>;

    fn call(&self, arg: Fallback<S>) -> Self::Output {
        let adapters = self.0.clone();
        Box::pin(async move {
            let mut fallback = arg;

            for adapter in adapters.iter() {
                fallback = match adapter
                    .call({
                        let _ = fallback.mark().await?;
                        fallback
                    })
                    .await?
                {
                    UnpackerAdapterOutcome::Accepted(acc) => {
                        return Ok(acc);
                    }
                    UnpackerAdapterOutcome::Unacceptable(fallback) => fallback,
                }
            }

            Ok(Peer::Unexpected(fallback))
        })
    }
}
