use std::{future::Future, pin::Pin};

use super::Accepter;
use crate::Result;

pub struct Accept<'a, T> {
    accepter: &'a mut T,
}

pub trait AccepterExt: Accepter {
    fn accept<'a>(&'a mut self) -> Accept<'a, Self>
    where
        Self: Sized + Unpin,
    {
        Accept { accepter: self }
    }
}

impl<'a, T> Future for Accept<'a, T>
where
    T: Accepter + Unpin,
{
    type Output = Result<T::Stream>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Pin::new(&mut *self.accepter).poll_accept(cx)
    }
}


impl<T> AccepterExt for T where T: Accepter + Unpin {}
