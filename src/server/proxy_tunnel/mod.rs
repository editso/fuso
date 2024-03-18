use crate::{core::accepter::Accepter, error};

pub struct ProxyTunnel {}

impl ProxyTunnel {
    pub fn new() -> Self {
        unimplemented!()
    }
}

impl Accepter for ProxyTunnel {
    type Output = error::Result<()>;
    fn poll_accept(
        self: std::pin::Pin<&mut Self>,
        ctx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<error::Result<Self::Output>> {
        unimplemented!()
    }
}
