use std::{pin::Pin, sync::Arc};

use crate::{
    guard::Fallback, server::Server, Accepter, Executor, Fuso, Provider, WrappedProvider, Socket,
    Stream,
};

use super::{
    server::{Peer, PenetrateProvider},
    PenetrateServerBuilder, PenetrateObserver,
};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub enum Selector<O> {
    Checked(Peer<Fallback<O>>),
    Unselected(Fallback<O>),
}

pub struct PenetrateSelector<A>(Arc<Vec<A>>);

pub struct PenetrateSelectorBuilder<E, P, S, O> {
    pub(crate) adapters: Vec<WrappedProvider<(Fallback<S>, Arc<super::server::Config>), Selector<S>>>,
    pub(crate) penetrate_builder: PenetrateServerBuilder<E, P, S, O>,
}

impl<E, P, S, O> PenetrateServerBuilder<E, P, S, O> {
    pub fn using_adapter(self) -> PenetrateSelectorBuilder<E, P, S, O> {
        PenetrateSelectorBuilder {
            adapters: Default::default(),
            penetrate_builder: self,
        }
    }
}

impl<E, A, P, S, O> PenetrateSelectorBuilder<E, P, S, O>
where
    E: Executor + 'static,
    A: Accepter<Stream = S> + Unpin + Send + 'static,
    S: Stream + Send + Sync + 'static,
    P: Provider<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
    O: PenetrateObserver + Send + Sync + 'static
{
    pub fn build(self) -> Fuso<Server<E, PenetrateProvider<S>, P, S, O>> {
        self.penetrate_builder
            .disable_fallback_strict_mode()
            .build(PenetrateSelector(Arc::new(self.adapters)))
    }
}

impl<S, M> Provider<(Fallback<S>, Arc<super::server::Config>)> for PenetrateSelector<M>
where
    S: Stream + Send + Unpin + 'static,
    M: Provider<(Fallback<S>, Arc<super::server::Config>), Output = BoxedFuture<Selector<S>>> + Sync + Send + 'static,
{
    type Output = BoxedFuture<Peer<Fallback<S>>>;

    fn call(&self, (fallback, config): (Fallback<S>, Arc<super::server::Config>)) -> Self::Output {
        let adapters = self.0.clone();
        Box::pin(async move {
            let mut fallback = fallback;

            for adapter in adapters.iter() {
                fallback.mark().await?;

                fallback = match adapter.call((fallback, config.clone())).await? {
                    Selector::Unselected(fallback) => fallback,
                    Selector::Checked(peer) => {
                        return Ok(peer);
                    }
                };

                fallback.backward().await?;
            }

            Ok(Peer::Unknown(fallback))
        })
    }
}
