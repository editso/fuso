use fuso_api::{Buffer, Rollback};

use crate::{
    core::SyncContext,
    dispatch::{Handler, State},
};
use async_trait::async_trait;

pub struct ForwardHandler {}


#[async_trait]
impl<T, IO, H> Handler<Rollback<T, Buffer<u8>>, SyncContext<IO, H>> for ForwardHandler
where
    H: Handler<Rollback<Self, Buffer<u8>>, SyncContext<IO, H>> + Send + Sync + 'static,
    IO: Send + Sync + 'static,
    T: Sync + Send + 'static,
{
    async fn dispose(
        &self,
        roll: Rollback<T, Buffer<u8>>,
        c: SyncContext<IO, H>,
    ) -> fuso_api::Result<State> {
        // roll.read().await;;
        

        todo!()
    }
}
