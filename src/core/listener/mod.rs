pub mod ext;

use crate::Result;

use std::{
    pin::Pin,
    task::{Context, Poll},
};

pub trait Accepter {
    type Stream;
    fn poll_accept(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<Self::Stream>>;
}
