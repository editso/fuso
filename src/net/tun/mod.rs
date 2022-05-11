use std::{
    borrow::Borrow, future::Future, marker::PhantomData, pin::Pin, rc::Rc, sync::Arc, task::Poll,
};

use crate::{
    connector::{ext::ConnectorExt, Connector},
    guard::Fallback,
    handler::{Handler, Outcome},
    listener::{ext::AccepterExt, Accepter},
    packet::AsyncRecvPacket,
    request::Request,
    service::Service,
    Addr, AsyncRead, AsyncWrite, Executor,
};

pub type BoxedService<E, S, A, C> = Box<
    dyn Service<
            Config = (E, Addr),
            Socket = Addr,
            Stream = S,
            Accepter = A,
            Connector = C,
            Future = Pin<Box<dyn Future<Output = crate::Result<(A, C)>> + Send + 'static>>,
        > + Send
        + 'static,
>;

pub struct Tun<P> {
    provider: P,
}

impl<S, E, A, C, F, P> Tun<P>
where
    P: Service<
        Config = (E, Addr),
        Socket = Addr,
        Stream = S,
        Accepter = A,
        Connector = C,
        Future = F,
    >,
    F: Future<Output = crate::Result<(A, C)>> + Send + 'static,
{
    pub fn new(provider: P) -> Self {
        Self { provider }
    }
}

impl<S, E, A, C, F, P> Handler for Tun<P>
where
    P: Service<
            Config = (E, Addr),
            Socket = Addr,
            Stream = S,
            Accepter = A,
            Connector = C,
            Future = F,
        > + Send
        + Sync
        + Clone
        + 'static,
    E: Executor + Sync + Clone + Send + 'static,
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    A: Accepter<Stream = P::Stream> + Unpin + Send + 'static,
    C: Connector<Socket = P::Socket, Stream = P::Stream> + Clone + Unpin + Send + 'static,
    F: Future<Output = crate::Result<(A, C)>> + Send + 'static,
{
    type Stream = S;
    type Executor = E;

    fn can_handle<'a>(
        &self,
        stream: &'a mut Fallback<Self::Stream>,
    ) -> Pin<Box<dyn Future<Output = crate::Result<Outcome>> + Send + 'a>> {
        Box::pin(async {
            let a = stream.recv_packet().await?;

            Ok(Outcome::Reject)
        })
    }

    fn handle(
        &self,
        req: Request<Self::Stream>,
        executor: Self::Executor,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<()>> + Send + 'static>>
    {
        let provider = self.provider.clone();

        Box::pin(async move {
            let e2 = executor.clone();

            let provider = provider
                .create(&(executor.clone(), "".parse().unwrap()))
                .await;
            
            let (mut accepter, connector) = provider?;

            loop {
                let stream = accepter.accept().await?;

                let connector = connector.clone();

                executor.spawn(async move {
                    let mut connector = connector;
                    let a = connector
                        .connect(&Addr::Socket("".parse().unwrap()))
                        .await
                        .unwrap();
                });
            }
        })
    }
}
