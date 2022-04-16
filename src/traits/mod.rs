use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::error::Result;

pub trait ToVec {
    fn to_vec(&self) -> Vec<u8>;
}

pub trait Executor {
    type Output;
    fn spawn<F>(&self, fut: F) -> Self::Output
    where
        F: Future<Output = Self::Output> + Send + 'static;
}

pub trait Register {
    type Output;
    type Metadata;
    fn poll_register(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        metadata: &Self::Metadata,
    ) -> Poll<Result<Self::Output>>;
}

pub trait Accepter {
    type Stream;

    fn poll_accept(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<Self::Stream>>;
}

pub trait Proxy {
    type Stream;

    fn poll_connect(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<Self::Stream>>;

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>>;
}

pub trait Decrypt {
    fn poll_decrypt_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize>>;
}

pub trait Encrypt {
    fn poll_encrypt_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<()>>;
}
