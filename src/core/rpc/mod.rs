mod callee;
mod caller;
mod keep;
mod polling;

pub mod structs;

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use crate::error;

pub use callee::*;
pub use caller::*;
pub use polling::*;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
enum Cmd {
    Ping,
    Pong,
    Cancel(u64),
    Transact { token: u64, packet: Vec<u8> },
}

#[derive(Debug, Serialize, Deserialize)]
enum Transact {
    Cancel(u64),
    Request(u64, Vec<u8>),
}

pub trait AsyncCall<T> {
    type Output<'a>
    where
        Self: 'a;

    fn call<'a>(&'a mut self, arg: T) -> Self::Output<'a>;
}

pub trait AsyncCallee {
    type Output;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>;
}

pub trait Encoder<T> {
    fn encode(self) -> error::Result<Vec<u8>>;

    fn borrow_encode(&self) -> error::Result<Vec<u8>>;
}

pub trait Decoder<T> {
    type Output;
    fn decode(self) -> error::Result<Self::Output>;
}

impl<T> Encoder<T> for T
where
    T: serde::Serialize,
{
    fn encode(self) -> error::Result<Vec<u8>> {
        self.borrow_encode()
    }

    fn borrow_encode(&self) -> error::Result<Vec<u8>> {
        rmp_serde::to_vec_named(self).map_err(Into::into)
    }
}

impl<T> Decoder<T> for Vec<u8>
where
    T: serde::de::DeserializeOwned,
{
    type Output = T;

    fn decode(self) -> error::Result<Self::Output> {
        rmp_serde::from_slice(&self).map_err(Into::into)
    }
}
