use std::{pin::Pin, sync::Arc, task::Poll};

use async_mutex::Mutex;
use std::future::Future;

use crate::{
    listener::{ext::AccepterExt, Accepter},
    protocol::{AsyncRecvPacket, AsyncSendPacket},
    select::Select,
    Stream,
};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub struct Penetrate<T, A> {
    target: Arc<Mutex<T>>,
    accepter: Arc<Mutex<A>>,
    recv_fut: Arc<Mutex<BoxedFuture<(T, T)>>>,
    write_fut: Arc<Mutex<BoxedFuture<(T, T)>>>,
    accepter_fut: Option<BoxedFuture<(T, T)>>,
}

impl<T, A> Penetrate<T, A>
where
    T: Stream + Send + 'static,
    A: Accepter<Stream = T> + Unpin + Send + 'static,
{
    pub fn new(target: T, accepter: A) -> Self {
        let target = Arc::new(Mutex::new(target));
        Self {
            target: target.clone(),
            accepter: Arc::new(Mutex::new(accepter)),
            recv_fut: Arc::new(Mutex::new(Box::pin(Self::poll_handle_recv(target.clone())))),
            write_fut: Arc::new(Mutex::new(Box::pin(Self::poll_heartbeat_future(target)))),
            accepter_fut: None,
        }
    }

    async fn poll_handle_recv(mut stream: Arc<Mutex<T>>) -> crate::Result<(T, T)> {
        loop {
            let packet = stream.lock().await.recv_packet().await?;
        }
    }

    async fn poll_heartbeat_future(mut stream: Arc<Mutex<T>>) -> crate::Result<(T, T)> {
        loop {
            // sleep
            let mut stream = stream.lock().await;
            stream.send_packet(b"").await?;



        }
    }

    async fn poll_accept_future(
        stream: Arc<Mutex<T>>,
        accepter: Arc<Mutex<A>>,
    ) -> crate::Result<(T, T)> {
        loop {
            let mut accept_tx = accepter.lock().await.accept().await?;
            let tx = accept_tx.recv_packet().await?;
            let a = stream.lock().await.send_packet(b"connect").await?;

            

        }
    }
}

impl<T, A> Accepter for Penetrate<T, A>
where
    T: Stream + Send + 'static,
    A: Accepter<Stream = T> + Unpin + Send + 'static,
{
    type Stream = (T, T);

    fn poll_accept(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<Self::Stream>> {
        let target = self.target.clone();
        let accepter = self.accepter.clone();
        let recv_fut = self.recv_fut.clone();
        let write_fut = self.write_fut.clone();

        let mut fut = match self.accepter_fut.take() {
            Some(fut) => fut,
            None => Box::pin(
                Select::select(
                    async move { Pin::new(&mut *recv_fut.lock().await).await },
                    async move { Pin::new(&mut *write_fut.lock().await).await },
                )
                .add(Penetrate::<T, A>::poll_accept_future(target, accepter)),
            ),
        };

        match Pin::new(&mut fut).poll(cx) {
            Poll::Ready(ready) => Poll::Ready(ready),
            Poll::Pending => {
                drop(std::mem::replace(&mut self.accepter_fut, Some(fut)));
                Poll::Pending
            }
        }
    }
}
