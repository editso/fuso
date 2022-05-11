use std::{future::Future, marker::PhantomData, panic::AssertUnwindSafe, pin::Pin, sync::Arc};

use crate::{
    guard::{ext::StreamGuardExt, Fallback, StreamGuard},
    handler::Outcome,
    listener::{ext::AccepterExt, Accepter},
    request::Request,
    AsyncRead, AsyncWrite, BoxedFuture, Executor,
};

use super::builder::FusoServer;

pub struct ServerInner<S, E> {
    _marked: PhantomData<(S, E)>,
}

pub struct Server<S, E> {
    fut: BoxedFuture<'static, crate::Result<()>>,
    _marked: PhantomData<(S, E)>,
}

impl<S, E> Server<S, E>
where
    S: AsyncWrite + AsyncRead + Send + Unpin + 'static,
    E: Executor + Clone + Send + 'static,
{
    pub fn new<A>(accepter: A, builder: FusoServer<S, E>) -> Self
    where
        A: Accepter<Stream = S> + Unpin + Send + 'static,
    {
        Server {
            fut: Box::pin(ServerInner::run(accepter, builder)),
            _marked: PhantomData,
        }
    }
}

impl<E, S> Future for Server<S, E>
where
    E: Unpin + 'static,
    S: Unpin + 'static,
{
    type Output = crate::Result<()>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Pin::new(&mut Pin::into_inner(self).fut).poll(cx)
    }
}

impl<E, S> ServerInner<S, E>
where
    E: Executor + Clone + Send + 'static,
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    async fn run<A>(mut accepter: A, builder: FusoServer<S, E>) -> crate::Result<()>
    where
        A: Accepter<Stream = S> + Unpin + Send + 'static,
    {
        log::debug!("started !");

        let executor = builder.executor.clone();
        let services = Arc::new(builder.services);

        loop {
            let stream = accepter.accept().await?;

            let services = services.clone();

            let fut = {
                let executor = executor.clone();
                async move {
                    let mut stream = Fallback::new(stream);
                    let mut services = services.iter();

                    loop {
                        let _ = stream.ward().await;

                        if let Some(handler) = services.next() {
                            match handler.can_handle(&mut stream).await {
                                Ok(Outcome::Agree) => {
                                    let stream = stream.into_inner();
                                    let req = Request::new(stream);
                                    executor.spawn(handler.handle(req, executor.clone()));
                                    break;
                                }
                                Err(e) => {
                                    log::error!("{:?}", e);
                                    break;
                                }
                                _ => {}
                            }
                        } else {
                            log::warn!("received an invalid connection");
                            break;
                        }

                        let _ = stream.backward().await;
                    }
                }
            };

            if let Err(e) = std::panic::catch_unwind(AssertUnwindSafe(|| executor.spawn(fut))) {
                log::error!("{:?}", e);
            }
        }
    }
}
