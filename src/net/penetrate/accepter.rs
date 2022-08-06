use std::{pin::Pin, task::Poll};

use crate::{Accepter, Address, NetSocket, Stream};

pub enum Pen<S> {
    Visit(S),
    Client(S),
}

pub struct PenetrateAccepter<CA, SA> {
    visit: SA,
    client: CA,
}

impl<CA, SA, S> PenetrateAccepter<CA, SA>
where
    CA: Accepter<Stream = S> + Unpin + 'static,
    SA: Accepter<Stream = S> + Unpin + 'static,
{
    pub fn new(visit: SA, client: CA) -> Self {
        Self { visit, client }
    }
}

impl<CA, SA> NetSocket for PenetrateAccepter<CA, SA>
where
    CA: NetSocket,
    SA: NetSocket,
{
    fn local_addr(&self) -> crate::Result<crate::Address> {
        Ok(self.visit.local_addr()? + self.client.local_addr()?)
    }

    fn peer_addr(&self) -> crate::Result<Address> {
        Ok(self.visit.peer_addr()? + self.client.peer_addr()?)
    }
}

impl<CA, SA, S> Accepter for PenetrateAccepter<CA, SA>
where
    CA: Accepter<Stream = S> + Unpin,
    SA: Accepter<Stream = S> + Unpin,
    S: Stream,
{
    type Stream = Pen<S>;

    fn poll_accept(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<Self::Stream>> {
        let mut poll_next = true;

        while poll_next {
            match Pin::new(&mut self.visit).poll_accept(cx)? {
                std::task::Poll::Ready(visit) => return Poll::Ready(Ok(Pen::Visit(visit))),
                std::task::Poll::Pending => {}
            }

            match Pin::new(&mut self.client).poll_accept(cx)? {
                std::task::Poll::Ready(client) => return Poll::Ready(Ok(Pen::Client(client))),
                std::task::Poll::Pending => {
                    poll_next = false;
                }
            }
        }

        return Poll::Pending;
    }
}
