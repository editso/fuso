use std::{collections::HashMap, marker::PhantomData, pin::Pin, sync::Arc};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::{
    core::{
        future::Select,
        protocol::{AsyncPacketRead, AsyncPacketSend},
        rpc::channel,
        split::{ReadHalf, WriteHalf},
        task::{setter, Setter},
        token::IncToken,
        Stream,
    },
    error,
};

use super::{
    channel::{Receiver, Sender},
    AsyncCall, Decoder,
};
use crate::core::rpc::Encoder;
use crate::core::split::SplitStream;

#[derive(Debug, Serialize, Deserialize)]
struct Request {
    token: u64,
    data: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Response {
    token: u64,
    data: Vec<u8>,
}

pub struct Reactor<'a>(Select<'a, error::Result<()>>);

#[derive(Default, Clone)]
pub struct Calls {
    call_list: Arc<Mutex<HashMap<u64, Setter<Vec<u8>>>>>,
    inc_token: IncToken,
}

pub struct Caller<'a, S> {
    calls: Calls,
    request: Sender<Request>,
    marked: PhantomData<(&'a (), S)>,
}

impl<'a, S> Caller<'a, S>
where
    S: Stream + Send + Unpin + 'a,
{
    pub fn new(stream: S) -> (Reactor<'a>, Self) {
        let (reader, writer) = stream.split();
        let (req_rx, req_ax) = channel::open::<Request>();

        let calls = Calls::default();
        let mut select = Select::new();

        select.add(Reactor::run_recv_looper(calls.clone(), writer, req_ax));
        select.add(Reactor::run_send_looper(calls.clone(), reader));

        (
            Reactor(select),
            Self {
                calls,
                request: req_rx,
                marked: PhantomData,
            },
        )
    }
}

impl<'caller, S, T> AsyncCall<T> for Caller<'caller, S>
where
    T: serde::Serialize + Send + 'static,
    S: Send + Unpin + 'caller,
{
    type Output = error::Result<Vec<u8>>;

    fn call<'a>(&'a mut self, data: T) -> crate::core::BoxedFuture<'a, Self::Output> {
        let data = data.encode();

        Box::pin(async move {
            let data = data?;

            let (setter, getter) = setter();

            let token = self.calls.add(setter);

            match self.request.send(Request { token, data }).await {
                Ok(()) => getter.await,
                Err(e) => {
                    self.calls.cancel(token);
                    Err(e)
                }
            }
        })
    }
}

impl<'a> Reactor<'a> {
    async fn run_recv_looper<S>(
        calls: Calls,
        writer: WriteHalf<S>,
        receiver: Receiver<Request>,
    ) -> error::Result<()>
    where
        S: Stream + Unpin,
    {
        let mut writer = writer;
        loop {
            let pkt = receiver.recv().await?.encode()?;
            if let Err(_) = writer.send_packet(&pkt).await {
                calls.cancel_all();
            };
        }
    }

    async fn run_send_looper<S>(calls: Calls, reader: ReadHalf<S>) -> error::Result<()>
    where
        S: Stream + Unpin,
    {
        let mut reader = reader;
        loop {
            let data = reader.recv_packet().await?;

            if let Err(e) = calls.wake(data.decode()?) {
                log::warn!("{:?}", e);
            }
        }
    }
}

impl<'a> std::future::Future for Reactor<'a> {
    type Output = error::Result<()>;
    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}

impl Calls {
    fn add(&self, setter: Setter<Vec<u8>>) -> u64 {
        let mut calls = self.call_list.lock();

        let token = self.inc_token.next(|token| !calls.contains_key(&token));

        calls.insert(token, setter);

        token
    }

    fn wake(&self, Response { token, data }: Response) -> error::Result<()> {
        match self.call_list.lock().remove(&token) {
            None => Err(error::FusoError::BadRpcCall(token)),
            Some(setter) => setter.set(data),
        }
    }

    fn cancel(&self, token: u64) {
        drop(self.call_list.lock().remove(&token))
    }

    fn cancel_all(&self) {
        self.call_list.lock().clear();
    }
}

impl<T> Clone for Caller<'_, T> {
    fn clone(&self) -> Self {
        Caller {
            calls: self.calls.clone(),
            request: self.request.clone(),
            marked: PhantomData,
        }
    }
}
