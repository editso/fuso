#[allow(unused)]

mod stream;
use stream::*;

use crate::{
    protocol::AsyncRecvPacket, sync::Mutex, FusoStream, Provider, Socket, WrappedProvider,
};
use std::{collections::VecDeque, future::Future, io, pin::Pin, sync::Arc, task::Poll};

use crate::{AsyncRead, AsyncWrite, Executor, Kind, Task};

type BoxedFuture<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + Send + 'static>>;

macro_rules! reset {
    () => {
        io::Error::from(io::ErrorKind::ConnectionReset).into()
    };
}

enum RW {
    Read,
    Write,
}

pub struct ConnectionPool<E, C, S> {
    executor: E,
    connector: C,
    guard_task: Task<()>,
    connections: Arc<Mutex<VecDeque<(PoolStream<S>, RW)>>>,
}

impl<E, C, S> ConnectionPool<E, C, S>
where
    E: Executor + Send + 'static,
    S: AsyncRead + AsyncWrite + Unpin + 'static,
{
    pub fn new(executor: E) -> Self {
        let connections: Arc<Mutex<VecDeque<(PoolStream<S>, RW)>>> = Default::default();

        let guard_task = {
            let connections = connections.clone();
            executor.spawn(async move {
                let mut connections = connections.lock().await;
                for (connection, rw) in connections.iter_mut() {
                    match rw {
                        RW::Read => todo!(),
                        RW::Write => todo!(),
                    }
                }
            })
        };

        Self {
            executor,
            guard_task,
            connector: todo!(),
            connections,
        }
    }
}



impl<E, S> Provider<Socket> for ConnectionPool<E, WrappedProvider<Socket, S>, S>
where S: Send + 'static{
    type Output = BoxedFuture<FusoStream>;
    fn call(&self, socket: Socket) -> Self::Output {
        let connector = self.connector.clone();
        let connections = self.connections.clone();
        Box::pin(async move {
            match connections.lock().await.pop_front() {
                Some((stream, rw)) => {
                    unimplemented!()
                }
                None => {
                    let socket = connector.call(socket).await?;
                    
                }
            }

            unimplemented!()
        })
    }
}
