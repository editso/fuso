use std::{future::Future, pin::Pin, sync::Arc};

use crate::Address;

type BoxedFuture<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + Send + 'static>>;

pub trait Controller {
    fn register(
        &self,
        env: Arc<dyn Environ + Send + Sync + 'static>,
        fut: BoxedFuture<()>,
    ) -> crate::Result<()>;
}

pub trait Environ {
    fn conn(&self) -> Address;

    fn info(&self) -> crate::Result<serde_json::Value>;
}

impl<C> Controller for Arc<C>
where
    C: Controller,
{
    #[inline]
    fn register(
        &self,
        env: Arc<dyn Environ + Send + Sync + 'static>,
        fut: BoxedFuture<()>,
    ) -> crate::Result<()> {
        (**self).register(env, fut)
    }
}

impl<E> Environ for Arc<E>
where
    E: Environ,
{
    fn conn(&self) -> Address {
        (**self).conn()
    }

    fn info(&self) -> crate::Result<serde_json::Value> {
        (**self).info()
    }
}
