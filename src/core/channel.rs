use std::{
    collections::VecDeque,
    sync::Arc,
    task::{Context, Poll, Waker},
};

use parking_lot::Mutex;

use crate::error;

struct Container<T> {
    buffer: Arc<Mutex<VecDeque<T>>>,
    waker: Arc<Mutex<Option<Waker>>>,
}

pub struct Sender<T> {
    container: Container<T>,
}

pub struct Receiver<T> {
    container: Container<T>,
}

pub struct Recv<'a, T> {
    container: &'a Container<T>,
}

pub struct Send<'a, T> {
    data: Option<T>,
    sender: &'a Container<T>,
}

impl<T> Receiver<T> {
    pub fn recv<'a>(&'a self) -> Recv<'a, T> {
        Recv {
            container: &self.container,
        }
    }

    pub fn poll_recv(&self, cx: &mut Context<'_>) -> Poll<error::Result<T>> {
        self.container.poll_take(cx)
    }
}

impl<T> Sender<T> {
    pub fn send<'a>(&'a self, data: T) -> Send<'a, T> {
        Send {
            sender: &self.container,
            data: Some(data),
        }
    }

    pub fn send_sync(&self, data: T) {
        self.container.push(data);
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            container: self.container.clone(),
        }
    }
}

impl<T> Default for Container<T> {
    fn default() -> Self {
        Self {
            buffer: Default::default(),
            waker: Default::default(),
        }
    }
}

impl<T> Clone for Container<T> {
    fn clone(&self) -> Self {
        Self {
            buffer: self.buffer.clone(),
            waker: self.waker.clone(),
        }
    }
}

pub fn open<T>() -> (Sender<T>, Receiver<T>) {
    let container = Container::default();

    (
        Sender {
            container: container.clone(),
        },
        Receiver { container },
    )
}

impl<'a, T> std::future::Future for Send<'a, T>
where
    T: Unpin,
{
    type Output = error::Result<()>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let data = self.data.take().expect("invalid state");
        self.sender.push(data);
        Poll::Ready(Ok(()))
    }
}

impl<'a, T> std::future::Future for Recv<'a, T> {
    type Output = error::Result<T>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.container.poll_take(cx)
    }
}

impl<T> Container<T> {
    fn push(&self, data: T) {
        let mut buf = self.buffer.lock();

        buf.push_back(data);

        if let Some(waker) = self.waker.lock().take() {
            waker.wake();
        }
    }

    fn take(&self, waker: &Waker) -> Option<T> {
        match self.buffer.lock().pop_front() {
            Some(data) => Some(data),
            None => {
                drop(self.waker.lock().replace(waker.clone()));
                None
            }
        }
    }

    pub fn poll_take(&self, cx: &mut Context<'_>) -> Poll<error::Result<T>> {
        match self.take(cx.waker()) {
            None => Poll::Pending,
            Some(t) => Poll::Ready(Ok(t)),
        }
    }
}
