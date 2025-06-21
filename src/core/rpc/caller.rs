use std::{collections::HashMap, marker::PhantomData, sync::Arc};

use parking_lot::Mutex;

use crate::{
    core::{
        channel::{Receiver, Sender},
        future::LazyFuture,
        task::{setter, Setter},
        token::IncToken,
        BoxedFuture, Stream,
    },
    error::{self, FusoError},
    runtime::Runtime,
};

use super::{polling::Looper, AsyncCall, Cmd, Transact};
use crate::core::rpc::Encoder;

#[derive(Default, Clone)]
pub struct Calls {
    call_list: Arc<Mutex<HashMap<u64, Setter<Vec<u8>>>>>,
    inc_token: IncToken,
}

pub struct Caller<S> {
    calls: Calls,
    request: Sender<Cmd>,
    caller: Arc<Mutex<LazyFuture<'static, error::Result<Vec<u8>>>>>,
    marked: PhantomData<S>,
}

impl<'a, S> Caller<S>
where
    S: Stream + Send + Unpin + 'a,
{
    pub fn new<R>(stream: S, heartbeat_delay: std::time::Duration) -> (Looper<'a>, Self)
    where
        R: Runtime + 'a,
    {
        let calls = Calls::default();

        let (mut looper, sender, receiver) = Looper::new(stream);

        looper.post(Looper::run_command_consumer::<R>(receiver, calls.clone()));

        (
            looper,
            Self {
                calls,
                request: sender,
                caller: Arc::new(Mutex::new(LazyFuture::new())),
                marked: PhantomData,
            },
        )
    }
}

impl<'caller, S, T> AsyncCall<T> for Caller<S>
where
    T: serde::Serialize + Send + 'static,
    S: Send + Unpin + 'caller,
{
    type Output<'a> = BoxedFuture<'a, error::Result<Vec<u8>>> where S: 'a;

    fn call<'a>(&'a mut self, arg: T) -> Self::Output<'a> {
        let data = arg.encode();
        let (setter, getter) = setter();
        let token = self.calls.add(setter);

        Box::pin(async move {
            let result = match data {
                Ok(packet) => self.request.send(Cmd::Transact { token, packet }).await,
                Err(error) => {
                    self.calls.cancel(token);
                    Err(error)
                }
            };

            match result {
                Err(e) => Err(e),
                Ok(_) => match getter.await {
                    Ok(o) => Ok(o),
                    Err(FusoError::InvaledSetter) => Err(FusoError::Cancel),
                    Err(e) => Err(e),
                },
            }
        })
    }
}

impl<'a> Looper<'a> {
    async fn run_command_consumer<R>(
        receiver: Receiver<Transact>,
        calls: Calls,
    ) -> error::Result<()>
    where
        R: Runtime,
    {
        loop {
            match receiver.recv().await? {
                Transact::Cancel(token) => {
                    calls.cancel(token);
                }
                Transact::Request(token, packet) => {
                    calls.wake(token, packet)?;
                }
            }
        }
    }
}

impl Calls {
    fn add(&self, setter: Setter<Vec<u8>>) -> u64 {
        let mut calls = self.call_list.lock();

        let token = self.inc_token.next(|token| !calls.contains_key(&token));

        calls.insert(token, setter);

        token
    }

    fn wake(&self, token: u64, packet: Vec<u8>) -> error::Result<()> {
        match self.call_list.lock().remove(&token) {
            None => Err(error::FusoError::BadRpcCall(token)),
            Some(setter) => setter.set(packet),
        }
    }

    fn cancel(&self, token: u64) {
        drop(self.call_list.lock().remove(&token))
    }
}

impl<T> Clone for Caller<T> {
    fn clone(&self) -> Self {
        Caller {
            calls: self.calls.clone(),
            request: self.request.clone(),
            marked: PhantomData,
            caller: self.caller.clone(),
        }
    }
}
