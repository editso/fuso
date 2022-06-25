pub mod ext;

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use crate::{Addr, Result};

pub trait Connector: Send {
    type Stream;

    fn poll_connect(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        socket: &Addr,
    ) -> Poll<Result<Self::Stream>>;
}
