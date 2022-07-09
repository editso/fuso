use std::pin::Pin;

use crate::{
    error,
    FactoryWrapper,
    guard::Fallback,
    penetrate::{
        server::{Peer, Visitor},
        Adapter,
    },
    Factory,
    Socket, Stream,
};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub struct SocksUnpacker;
pub struct UdpForwardFactory<S>(std::sync::Mutex<Option<S>>);

impl<S> Factory<Fallback<S>> for SocksUnpacker
where
    S: Stream + Send + 'static,
{
    type Output = BoxedFuture<Adapter<S>>;

    fn call(&self, stream: Fallback<S>) -> Self::Output {
        Box::pin(async move {
            Ok(Adapter::Accept(Peer::Visitor(
                Visitor::Consume({
                    let wrap = UdpForwardFactory(std::sync::Mutex::new(Some(stream)));
                    FactoryWrapper::wrap(wrap)
                }),
                Socket::Udp(10.into()),
            )))
        })
    }
}

impl<S> Factory<Fallback<S>> for UdpForwardFactory<Fallback<S>>
where
    S: Stream + Send + 'static,
{
    type Output = BoxedFuture<()>;

    fn call(&self, s1: Fallback<S>) -> Self::Output {
        let lock = self.0.lock();

        if lock.is_err() {
            let err = error::Error::from(unsafe { lock.unwrap_err_unchecked() });
            return Box::pin(async move { Err(err) });
        }

        let s2 = unsafe { lock.unwrap_unchecked() }.take();

        let fut = async move {
            if s2.is_none() {
                return Err(error::Kind::AlreadyUsed.into());
            }

            let s1 = s1.into_inner();
            let s2 = unsafe { s2.unwrap_unchecked() }.into_inner();

            Ok(())
        };

        Box::pin(fut)
    }
}
