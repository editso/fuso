use std::{future::Future, pin::Pin};

use crate::{server::Server, Controller, Fuso};

type BoxedFuture<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + Send + 'static>>;

pub struct FusoApi {}

impl Controller for FusoApi {
    fn register(
        &self,
        env: std::sync::Arc<dyn crate::Environ + Send + Sync + 'static>,
        fut: BoxedFuture<()>,
    ) -> crate::Result<()> {
        unimplemented!()
    }
}

impl<E, H, P, S, O> Fuso<Server<E, H, P, S, O>> {
    
}
