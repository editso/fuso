use crate::{
    core::{
        rpc::{
            structs::port_forward::{Request, Response},
            AsyncCall,
        },
        split::{ReadHalf, WriteHalf},
        BoxedFuture,
    },
    error,
};

pub struct Transport<T> {
    reader: ReadHalf<T>,
    writer: WriteHalf<T>,
}

impl<T> Transport<T> {
    pub fn new(reader: ReadHalf<T>, writer: WriteHalf<T>) -> Self {
        Self { reader, writer }
    }
}

impl<T> AsyncCall<Request> for Transport<T> {
    type Output = error::Result<Response>;

    fn call<'a>(&'a mut self, data: Request) -> BoxedFuture<'a, Self::Output> {
        Box::pin(async move { unimplemented!() })
    }
}

impl<T> Clone for Transport<T> {
    fn clone(&self) -> Self {
        Self {
            reader: self.reader.clone(),
            writer: self.writer.clone(),
        }
    }
}
