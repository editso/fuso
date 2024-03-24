use crate::{
    core::{processor::Preprocessor, rpc::structs::port_forward::VisitorProtocol},
    error,
};

use super::Connection;

impl Preprocessor<Connection> for () {
    type Output = error::Result<VisitorProtocol>;

    fn prepare<'a>(&'a self, input: Connection) -> crate::core::BoxedFuture<'a, Self::Output> {
        Box::pin(async move { Ok(VisitorProtocol::Other(input, None)) })
    }
}

impl Preprocessor<Connection> for Option<()> {
    type Output = error::Result<Connection>;
    fn prepare<'a>(&'a self, input: Connection) -> crate::core::BoxedFuture<'a, Self::Output> {
        Box::pin(async move { Ok(input) })
    }
}
