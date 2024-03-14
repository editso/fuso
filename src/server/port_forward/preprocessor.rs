use crate::core::{processor::Preprocessor, rpc::structs::port_forward::VisitorProtocol};

use super::Connection;

impl Preprocessor<Connection> for () {
    type Output = VisitorProtocol;

    fn prepare<'a>(&'a self, input: Connection) -> crate::core::BoxedFuture<'a, Self::Output> {
        Box::pin(async move { VisitorProtocol::Other(input, None) })
    }
}
