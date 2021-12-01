use async_trait::async_trait;
use fuso_api::{Buffer, Rollback, RollbackEx};
use smol::net::TcpStream;

use crate::{core::SyncContext, split};

pub enum State {
    Next,
    Accept,
}

#[async_trait]
pub trait Handler<D, C> {
    async fn dispose(&self, o: D, c: C) -> fuso_api::Result<State>;
}

#[async_trait]
pub trait Dispatch<C> {
    async fn dispatch(self, cx: C) -> fuso_api::Result<()>;
}


#[async_trait]
impl<IO, H> Dispatch<SyncContext<IO, H>> for TcpStream
where
    H: Handler<Rollback<Self, Buffer<u8>>, SyncContext<IO, H>> + Send + Sync + 'static,
    IO: Send + Sync + 'static,
{
    async fn dispatch(self, cx: SyncContext<IO, H>) -> fuso_api::Result<()> {
        let (cx_1, cx_2) = split(cx);
        let cx_1 = cx_1.read().await;
        let handler = &cx_1.config.handler;

        let io = self.roll();

        io.begin().await?;

        for handle in handler.iter() {
            let handle = handle.clone();

            match handle.dispose(io.clone(), cx_2.clone()).await? {
                State::Accept => return Ok(()),
                State::Next => {
                    io.back().await?;
                    log::debug!("[dispatch] Next handler, rollback");
                }
            }
        }

        Err(fuso_api::ErrorKind::UnHandler.into())
    }
}
