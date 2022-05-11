use super::Connector;
use crate::Result;
use std::future::Future;

pub struct Connect<'a, C, T> {
    target: &'a T,
    connector: &'a mut C,
}

pub trait ConnectorExt: Connector {
    fn connect<'a>(&'a mut self, target: &'a Self::Socket) -> Connect<'a, Self, Self::Socket>
    where
        Self: Sized + Unpin,
    {
        Connect {
            connector: self,
            target,
        }
    }
}

impl<'a, C> Future for Connect<'a, C, C::Socket>
where
    C: Connector + Unpin,
{
    type Output = Result<C::Stream>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        // Pin::new(&mut *self.connector).poll_connect(cx, self.target)
        unimplemented!()
    }
}

impl<T> ConnectorExt for T where T: Connector + Unpin {}
