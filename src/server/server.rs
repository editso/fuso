use std::{pin::Pin, sync::Arc};

use crate::{
    core::listener::ext::AccepterExt,
    io,
    join::Join,
    listener::Accepter,
    middleware::FactoryTransfer,
    protocol::{AsyncRecvPacket, AsyncSendPacket, Behavior, Bind},
    service::{Factory, ServerFactory},
    Addr, Executor, Stream,
};

use super::{penetrate::Penetrate, BoxedEncryption};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub struct Server<E, SF, CF, SI> {
    pub bind: Addr,
    pub executor: E,
    pub factory: ServerFactory<SF, CF>,
    pub encryption: Vec<BoxedEncryption>,
    pub middleware: Option<Arc<FactoryTransfer<SI>>>,
}

impl<E, A, SF, CF, SI> Server<E, SF, CF, SI>
where
    E: Executor + Send + Clone + 'static,
    A: Accepter<Stream = SI> + Unpin + Send + 'static,
    SF: Factory<Addr, Output = BoxedFuture<A>> + Send + Sync + 'static,
    CF: Factory<Addr, Output = BoxedFuture<SI>> + Send + Sync + 'static,
    SI: Stream + Send + 'static,
{
    pub async fn run(self) -> crate::Result<()> {
        let mut accepter = self.factory.bind(self.bind.clone()).await?;

        loop {
            let stream = accepter.accept().await?;
            
            let executor = self.executor.clone();
            let middleware = self.middleware.clone();
            let factory = self.factory.clone();

            self.executor.spawn(async move {
                let stream = match middleware.as_ref() {
                    None => Ok(stream),
                    Some(factory) => factory.call(stream).await,
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
        stream: SI,
        executor: E,
        factory: ServerFactory<SF, CF>,
    ) -> crate::Result<()> {
        let mut stream = stream;

        let behavior = TryInto::<Behavior>::try_into(&stream.recv_packet().await?)?;

        let mut accepter = if let Behavior::Bind(Bind::Bind(addr)) = behavior {
            match factory.bind(addr.clone()).await {
                Ok(accepter) => Penetrate::new(stream, accepter),
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
            let (from, to) = accepter.accept().await?;

            executor.spawn(Join::join(
                async move {
                    let _ = io::copy(from, to).await;
                },
                async move {
                    // let _ = io::copy(to, from).await;
                    unimplemented!()
                },
            ));
        }
    }
}
