use std::sync::Arc;

use crate::{
    encryption::Encryption,
    listener::Accepter,
    service::{Factory, ServerFactory},
    Addr, AsyncRead, AsyncWrite, BoxedFuture, Executor, Fuso,
};

pub trait Stream: AsyncRead + AsyncWrite {}

impl<S> Stream for S where S: AsyncRead + AsyncWrite {}

#[derive(Clone)]
pub struct BoxedEncryption();

#[derive(Default)]
pub struct Builder {
    encryption: Vec<BoxedEncryption>,
}

impl Builder {
    pub fn filter(mut self) -> Self {
        self
    }

    pub fn add_encryption<T>(mut self, encryption: T) -> Self
    where
        T: Into<BoxedEncryption>,
    {
        self.encryption.push(encryption.into());
        self
    }

    pub fn wrap(self) -> Self{
        self
    }

    pub fn build<E, SF, CF, S, O>(
        self,
        executor: E,
        factory: ServerFactory<SF, CF>,
    ) -> Fuso<super::Server<E, SF, CF>>
    where
        SF: Factory<Addr, Output = S, Future = BoxedFuture<'static, crate::Result<S>>> + 'static,
        CF: Factory<Addr, Output = O, Future = BoxedFuture<'static, crate::Result<O>>> + 'static,
        E: Executor + 'static,
        S: Accepter<Stream = O> + 'static,
        O: Stream + 'static,
    {
        Fuso(super::Server {
            executor,
            factory,
            bind: ([0, 0, 0, 0], 0).into(),
            encryption: self.encryption,
        })
    }
}
