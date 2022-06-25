use std::time::Duration;

use crate::{
    core::listener::ext::AccepterExt,
    guard::Timer,
    listener::Accepter,
    packet::AsyncRecvPacket,
    service::{Factory, ServerFactory},
    Addr, BoxedFuture, Executor, Stream,
};

use super::BoxedEncryption;

pub struct Server<E, S, C> {
    pub bind: Addr,
    pub executor: E,
    pub factory: ServerFactory<S, C>,
    pub encryption: Vec<BoxedEncryption>,
    pub middleware: Option<usize>
}

impl<E, SF, CF, S, O> Server<E, SF, CF>
where
    E: Executor + Clone + 'static,
    SF: Factory<Addr, Output = S, Future = BoxedFuture<'static, crate::Result<S>>> + 'static,
    CF: Factory<Addr, Output = O, Future = BoxedFuture<'static, crate::Result<O>>> + 'static,
    S: Accepter<Stream = O> + Unpin + 'static,
    O: Stream + Send + 'static,
{

    pub fn wrap(self, middleware: usize) -> Self{
        self
    }

    pub async fn run(self) -> crate::Result<()> {
        let executor = self.executor.clone();
        let factory = self.factory.clone();
        let mut accepter = self.factory.bind(self.bind.clone()).await?;
        loop {
            let stream = accepter.accept().await?;

            log::debug!("....");
            executor.spawn(async move {
                let mut stream = Timer::with_read_write(stream, Duration::from_secs(30));

               
            })
        }
    }
}
