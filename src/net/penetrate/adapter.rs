use std::{pin::Pin, sync::Arc};

use crate::{
    guard::Fallback, server::Server, Accepter, Executor, Provider, ProviderWrapper, Fuso, Socket,
    Stream,
};

use super::{
    server::{Peer, PenetrateProvider},
    PenetrateServerBuilder,
};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub enum Adapter<O> {
    Accept(Peer<Fallback<O>>),
    Reject(Fallback<O>),
}

pub struct PenetrateAdapter<A>(Arc<Vec<A>>);

pub struct PenetrateAdapterBuilder<E, SF, CF, S> {
    pub(crate) adapters: Vec<ProviderWrapper<Fallback<S>, Adapter<S>>>,
    pub(crate) penetrate_builder: PenetrateServerBuilder<E, SF, CF, S>,
}

impl<E, SF, CF, S> PenetrateServerBuilder<E, SF, CF, S> {
    pub fn with_adapter_mode(self) -> PenetrateAdapterBuilder<E, SF, CF, S> {
        PenetrateAdapterBuilder {
            adapters: Default::default(),
            penetrate_builder: self,
        }
    }
}

impl<E, A, SF, CF, S> PenetrateAdapterBuilder<E, SF, CF, S>
where
    E: Executor + 'static,
    SF: Provider<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
    CF: Provider<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    A: Accepter<Stream = S> + Unpin + Send + 'static,
    S: Stream + Send + Sync + 'static,
{
    pub fn build(self) -> Fuso<Server<E, PenetrateProvider<S>, SF, CF, S>> {
        self.penetrate_builder
            .disable_fallback_strict_mode()
            .build(PenetrateAdapter(Arc::new(self.adapters)))
    }
}

impl<S, A> Provider<Fallback<S>> for PenetrateAdapter<A>
where
    S: Stream + Send + Unpin + 'static,
    A: Provider<Fallback<S>, Output = BoxedFuture<Adapter<S>>> + Sync + Send + 'static,
{
    type Output = BoxedFuture<Peer<Fallback<S>>>;

    fn call(&self, arg: Fallback<S>) -> Self::Output {
        let adapters = self.0.clone();
        Box::pin(async move {
            let mut fallback = arg;

            for adapter in adapters.iter() {
                fallback.mark().await?;

                fallback = match adapter.call(fallback).await? {
                    Adapter::Reject(fallback) => fallback,
                    Adapter::Accept(peer) => {
                        return Ok(peer);
                    }
                };

                fallback.backward().await?;
            }

            Ok(Peer::Unknown(fallback))
        })
    }
}
