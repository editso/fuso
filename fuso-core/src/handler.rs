use std::{pin::Pin, sync::Arc};

use async_trait::async_trait;

use futures::Future;

use crate::dispatch::State;

pub type DynFuture<R> = dyn Future<Output = R> + Send + 'static;

pub struct ChainHandler<D, C, R> {
    chains: Vec<Arc<Box<dyn Fn(D, C) -> Pin<Box<DynFuture<R>>> + Send + Sync + 'static>>>,
}

impl<D, C, R> ChainHandler<D, C, R> {
    #[inline]
    pub fn new() -> Self {
        Self { chains: Vec::new() }
    }
}

impl<D, C, A> ChainHandler<D, C, fuso_api::Result<State<A>>> {
    pub fn next<Chain, Fut>(mut self, chain: Chain) -> Self
    where
        Chain: Fn(D, C) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = fuso_api::Result<State<A>>> + Send + 'static,
    {
        self.chains
            .push(Arc::new(Box::new(move |d, c| Box::pin(chain(d, c)))));
        self
    }
}

#[async_trait]
impl<D, C, A> crate::dispatch::Handler<D, C, A> for ChainHandler<D, C, fuso_api::Result<State<A>>>
where
    D: Clone + Send + Sync + 'static,
    C: Clone + Send + Sync + 'static,
{
    async fn dispose(&self, o: D, c: C) -> fuso_api::Result<State<A>> {
        let chains = self.chains.clone();

        for chain in chains.iter() {
            match chain(o.clone(), c.clone()).await? {
                State::Accept(a) => return Ok(State::Accept(a)),
                State::Release => return Ok(State::Release),
                State::Next => {}
            }
        }

        Ok(State::Next)
    }
}

#[test]
fn test_handler() {
    use crate::dispatch::Handler;

    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    smol::block_on(async move {
        let chains = ChainHandler::new();

        let state = chains
            .next(|_, _| async move {
                log::info!("[chian] chain 1");

                Ok(State::Next)
            })
            .next(|_, _| async move {
                log::info!("[chian] chain 2");
                Ok(State::Accept(()))
            })
            .dispose(10, 11)
            .await
            .unwrap();

        log::debug!("{:?}", state);
    });
}
