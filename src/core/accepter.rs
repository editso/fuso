use std::{
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

use crate::error;

use super::{BoxedStream, Stream};

pub trait Accepter {
    type Output;

    fn poll_accept(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<error::Result<Self::Output>>;
}

pub trait AccepterExt: Accepter {
    fn accept<'a>(&'a mut self) -> Accept<'a, Self>
    where
        Self: Sized + Unpin,
    {
        Accept { accepter: self }
    }
}

pub struct Accept<'a, A: Unpin> {
    accepter: &'a mut A,
}

pub struct BoxedAccepter<'a, O>(Box<dyn Accepter<Output = O> + Unpin + Send + 'a>);

pub struct StreamAccepter<'a, O>(Box<dyn Accepter<Output = O> + Unpin + Send + 'a>);

pub struct TaggedAccepter<'a, T: Clone, O> {
    tag: T,
    accepter: BoxedAccepter<'a, O>,
}

pub struct MultiAccepter<'a, O> {
    accepter_list: Vec<BoxedAccepter<'a, O>>,
}

impl<'a, O> BoxedAccepter<'a, O> {
    pub fn new<A>(accepter: A) -> Self
    where
        A: Accepter<Output = O> + Unpin + Send + 'a,
    {
        Self(Box::new(accepter))
    }
}

impl<'a, T, O> TaggedAccepter<'a, T, O>
where
    T: Clone,
{
    pub fn new<A>(tag: T, accepter: A) -> Self
    where
        A: Accepter<Output = O> + Unpin + Send + 'a,
    {
        Self {
            tag,
            accepter: BoxedAccepter::new(accepter),
        }
    }
}

impl<'a, O> MultiAccepter<'a, O> {
    pub fn new() -> Self {
        MultiAccepter {
            accepter_list: Default::default(),
        }
    }

    pub fn add<A>(&mut self, accepter: A)
    where
        A: Accepter<Output = O> + Unpin + Send + 'static,
    {
        self.accepter_list.push(BoxedAccepter::new(accepter))
    }
}

impl<'a, O> Accepter for MultiAccepter<'a, O> {
    type Output = O;

    fn poll_accept(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<error::Result<Self::Output>> {
        for accepter in &mut self.accepter_list {
            match Pin::new(accepter).poll_accept(ctx)? {
                Poll::Pending => continue,
                Poll::Ready(out) => return Poll::Ready(Ok(out)),
            }
        }

        Poll::Pending
    }
}

impl<'a, O> Accepter for BoxedAccepter<'a, O> {
    type Output = O;
    fn poll_accept(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<error::Result<Self::Output>> {
        Pin::new(&mut *self.0).poll_accept(ctx)
    }
}

impl<'a, T, O> Accepter for TaggedAccepter<'a, T, O>
where
    T: Clone + Unpin,
{
    type Output = (T, O);

    fn poll_accept(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<error::Result<Self::Output>> {
        match Pin::new(&mut self.accepter).poll_accept(ctx)? {
            Poll::Pending => Poll::Pending,
            Poll::Ready(o) => Poll::Ready(Ok((self.tag.clone(), o))),
        }
    }
}

impl<'a, A> std::future::Future for Accept<'a, A>
where
    A: Accepter + Unpin,
{
    type Output = error::Result<A::Output>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut *self.accepter).poll_accept(cx)
    }
}

impl<'a, O> StreamAccepter<'a, (SocketAddr, O)>
where
    O: Stream + 'static,
{
    pub fn new<A>(accepter: A) -> Self
    where
        A: Accepter<Output = (SocketAddr, O)> + Unpin + Send + 'a,
    {
        Self(Box::new(accepter))
    }
}

impl<'a, O> Accepter for StreamAccepter<'a, (SocketAddr, O)>
where
    O: Stream + Send + Unpin + 'static,
{
    type Output = (SocketAddr, BoxedStream<'a>);
    fn poll_accept(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<error::Result<Self::Output>> {
        match Pin::new(&mut *self.0).poll_accept(ctx)? {
            Poll::Pending => Poll::Pending,
            Poll::Ready((addr, stream)) => Poll::Ready(Ok((addr, BoxedStream::new(stream)))),
        }
    }
}

impl<A> AccepterExt for A where A: Accepter {}


#[cfg(test)]
mod tests {

    use super::{AccepterExt, MultiAccepter};

    #[tokio::test]
    async fn test_accepter() {
        let mut accepter = MultiAccepter::<(i32, i32)>::new();

        let (a, b) = accepter.accept().await.unwrap();
    }
}
                                                                                                                                                                                              