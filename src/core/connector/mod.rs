pub mod ext;

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use crate::{Addr, Result};

pub type BoxedConnector<Stream> = Box<dyn Connector<Socket = Addr, Stream = Stream>>;

pub trait Connector: Send{
    type Socket;
    type Stream;

    fn poll_connect(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        socket: &Self::Socket,
    ) -> Poll<Result<Self::Stream>>;
}
