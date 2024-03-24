pub mod caller;
pub mod channel;
pub mod structs;

use std::{pin::Pin, task::Poll};

use crate::error;

use super::BoxedFuture;

pub trait AsyncCall<T> {
    type Output;

    fn call<'a>(&'a mut self, data: T) -> BoxedFuture<'a, Self::Output>;
}

pub trait Encoder<T> {
    fn encode(self) -> error::Result<Vec<u8>>;
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
        Ok(bincode::serialize(&self).unwrap())
    }
}

impl<T> Decoder<T> for Vec<u8>
where
    T: serde::de::DeserializeOwned,
{
    type Output = T;

    fn decode(self) -> error::Result<Self::Output> {
        unimplemented!()
    }
}
