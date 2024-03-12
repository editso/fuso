pub mod structs;

use std::{pin::Pin, task::Poll};

use crate::error;

use super::BoxedFuture;

pub trait AsyncCall<T> {
    type Output;

    fn call<'a>(&'a mut self, data: T) -> BoxedFuture<'a, Self::Output>;
}
