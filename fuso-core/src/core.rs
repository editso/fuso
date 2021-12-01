use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use futures::{AsyncRead, AsyncWrite, TryStreamExt};
use smol::{
    channel::{unbounded, Receiver, Sender},
    lock::RwLock,
    net::TcpStream,
};

use fuso_api::{AsyncTcpSocketEx, Buffer, Result, Rollback, Spwan};

use crate::dispatch::{Dispatch, Handler};

pub type SyncContext<IO, H> = Arc<RwLock<Context<TcpStream, IO, H>>>;

#[derive(Debug, Clone)]
pub struct Config<H> {
    pub bind_addr: SocketAddr,
    pub debug: bool,
    pub handler: Vec<Arc<H>>,
}

pub struct Context<T, IO, H> {
    pub config: Arc<Config<H>>,
    pub sessions: Arc<RwLock<HashMap<u64, Sender<T>>>>,
    pub accept_ax: Sender<IO>,
}

pub struct FusoStream<IO> {
    from: IO,
    to: IO,
}

pub struct Fuso<IO> {
    accept_tx: Receiver<IO>,
}

impl<IO> Fuso<IO>
where
    IO: Send + Sync + 'static,
{
    pub async fn bind<H, T>(config: Config<H>) -> Result<Self>
    where
        H: Handler<Rollback<TcpStream, Buffer<u8>>, SyncContext<IO, H>> + Send + Sync + 'static,
        T: AsyncRead + AsyncWrite + Send + 'static,
    {
        let config = Arc::new(config);
        let (accept_ax, accept_tx) = unbounded();

        let bind_addr = config.bind_addr.clone();

        let cx = Arc::new(RwLock::new(Context {
            config,
            accept_ax,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }));

        async move {
            let _ = bind_addr
                .tcp_listen()
                .await
                .unwrap()
                .incoming()
                .try_fold(cx, |cx, stream| async move {
                    log::debug!("[tcp] accept {}", stream.local_addr().unwrap());

                    stream.dispatch(cx.clone()).detach();

                    Ok(cx)
                })
                .await;
        }
        .detach();

        Ok(Self { accept_tx })
    }
}


#[async_trait]
impl<IO> fuso_api::FusoListener<IO> for Fuso<IO>
where
    IO: Send + Sync + 'static,
{
    #[inline]
    async fn accept(&mut self) -> Result<IO> {
        self.accept_tx.recv().await.map_err(|e| e.into())
    }

    #[inline]
    async fn close(&mut self) -> Result<()> {
        self.accept_tx.close();
        Ok(())
    }
}
