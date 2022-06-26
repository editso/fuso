use std::{pin::Pin, sync::Arc, time::Duration, marker::PhantomData};

use crate::{
    core::listener::ext::AccepterExt,
    guard::{Fallback, Timer},
    listener::Accepter,
    middleware::{FactoryTransfer, FactoryWrapper},
    packet::{AsyncRecvPacket, AsyncSendPacket, Behavior, Bind},
    service::{Factory, ServerFactory, Transfer},
    Addr, FusoStream, Executor,
};

use super::BoxedEncryption;

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub struct Server<E, SF, CF, SI> {
    pub bind: Addr,
    pub executor: E,
    pub factory: ServerFactory<SF, CF>,
    pub encryption: Vec<BoxedEncryption>,
    pub middleware: Option<Arc<FactoryTransfer<FusoStream>>>,
    pub _marked: PhantomData<SI>
}

impl<E, A, SF, CF, SI> Server<E, SF, CF, SI>
where
    E: Executor + Send + Clone + 'static,
    A: Accepter<Stream = SI> + Unpin + Send + 'static,
    SF: Factory<Addr, Output = BoxedFuture<A>> + Send + Sync + 'static,
    CF: Factory<Addr, Output = BoxedFuture<SI>> + Send + Sync + 'static,
    SI: Transfer<Output = FusoStream> + Send + 'static
{
    pub async fn run(self) -> crate::Result<()> {
        let mut accepter = self.factory.bind(self.bind.clone()).await?;

        loop {
            let stream = accepter.accept().await?;
            log::debug!("[accepter::accept] handle connection");

            let executor = self.executor.clone();
            let middleware = self.middleware.clone();
            let factory = self.factory.clone();

            self.executor.spawn(async move {
                let stream = match middleware.as_ref() {
                    None => Ok(stream.transfer()),
                    Some(factory) => factory.call(stream.transfer()).await,
                };

                let err = match stream {
                    Err(e) => Err(e),
                    Ok(stream) => Self::spawn_run(stream, executor, factory).await,
                };

                if let Err(e) = err {
                    log::warn!("[accepter] {}", e);
                }
            })
        }
    }

    async fn spawn_run(
        stream: FusoStream,
        executor: E,
        factory: ServerFactory<SF, CF>,
    ) -> crate::Result<()> {
        let mut stream = Timer::with_read_write(stream, Duration::from_secs(30));

        let behavior = TryInto::<Behavior>::try_into(&stream.recv_packet().await?)?;

        let mut accepter = if let Behavior::Bind(Bind::Bind(addr)) = behavior {
            match factory.bind(addr.clone()).await {
                Ok(stream) => stream,
                Err(err) => {
                    let err = err.to_string().as_bytes().to_vec();
                    return stream
                        .send_packet(&Vec::from(&Behavior::Bind(Bind::Failed(addr, err))))
                        .await;
                }
            }
        } else {
            return Ok(());
        };

        loop {
    
            let stream = accepter.accept().await?.transfer();
            let mut fallback = Fallback::new(stream);
        }
    }
}
