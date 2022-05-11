use std::{future::Future, marker::PhantomData, pin::Pin};

pub struct Ward<'a, T, IO>(&'a mut T, PhantomData<IO>);
pub struct Backward<'a, T, IO>(&'a mut T, PhantomData<IO>);

pub trait StreamGuardExt<T>: super::StreamGuard<T>
where
    T: Unpin,
{
    fn ward<'a>(&'a mut self) -> Ward<'a, Self, T>
    where
        Self: Sized + Unpin,
    {
        Ward(self, PhantomData)
    }

    fn backward<'a>(&'a mut self) -> Backward<'a, Self, T>
    where
        Self: Sized + Unpin,
    {
        Backward(self, PhantomData)
    }
}

impl<T, IO> StreamGuardExt<IO> for T
where
    T: super::StreamGuard<IO>,
    IO: Unpin,
{
}

impl<'a, T, IO> Future for Ward<'a, T, IO>
where
    T: StreamGuardExt<IO> + Unpin,
    IO: Unpin,
{
    type Output = crate::Result<()>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Pin::new(&mut *self.0).poll_ward(cx)
    }
}

impl<'a, T, IO> Future for Backward<'a, T, IO>
where
    T: StreamGuardExt<IO> + Unpin,
    IO: Unpin,
{
    type Output = crate::Result<()>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Pin::new(&mut *self.0).poll_backward(cx)
    }
}
