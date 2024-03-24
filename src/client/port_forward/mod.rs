use env_logger::Target;

use crate::core::{accepter::Accepter, Stream};

pub struct PortForwarder {}

impl PortForwarder {
    pub fn new<S, C>(transport: S, connector: C) -> Self
    where
        S: Stream + Unpin,
    {
      unimplemented!()
    }
}

impl Accepter for PortForwarder {
    type Output = Target;

    fn poll_accept(
        self: std::pin::Pin<&mut Self>,
        ctx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::error::Result<Self::Output>> {
        todo!()
    }
}
