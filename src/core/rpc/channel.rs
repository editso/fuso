use std::{
    collections::VecDeque,
    sync::Arc,
    task::{Poll, Waker},
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
    receiver: &'a Container<T>,
}

pub struct Send<'a, T> {
    data: Option<T>,
    sender: &'a Container<T>,
}

impl<T> Receiver<T> {
    pub fn recv<'a>(&'a self) -> Recv<'a, T> {
        Recv {
            receiver: &self.container,
        }
    }
}

impl<T> Sender<T> {
    pub fn send<'a>(&'a self, data: T) -> Send<'a, T> {
        Send {
            sender: &self.container,
            data: Some(data),
        }
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
            waker: Default::default(),
        }
    }
}

pub fn open<T>() -> (Sender<T>, Receiver<T>) {
    let container = Container::default();

    (
        Sender {
            container: container.clone(),
        },
        Receiver {
            container: container,
        },
    )
}

impl<'a, T> std::future::Future for Send<'a, T>
where
    T: Unpin,
{
    type Output = error::Result<()>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
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
        match self.receiver.take(cx.waker()) {
            None => Poll::Pending,
            Some(data) => Poll::Ready(Ok(data)),
        }
    }
}

impl<T> Container<T> {
    fn push(&self, data: T) {
        self.buffer.lock().push_back(data);
        if let Some(waker) = self.waker.lock().take() {
            waker.wake();
        }
    }

    fn take(&self, waker: &Waker) -> Option<T> {
        match self.buffer.lock().pop_front() {
            Some(data) => Some(data),
            None => {
                self.waker.lock().replace(waker.clone());
                None
            }
        }
    }
}
