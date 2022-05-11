use std::{cell::RefCell, net::SocketAddr, rc::Rc};

use crate::RefContext;

use super::Server;

pub struct Request<S> {
    stream: S,
}

unsafe impl<S> Send for Request<S> {}

impl<S> Request<S>
where
    S: Send,
{
    pub fn new(stream: S) -> Self {
        Self { stream }
    }

    pub fn peer_addr(&self) -> SocketAddr {
        unimplemented!()
    }

    pub fn local_addr(&self) -> SocketAddr {
        unimplemented!()
    }
}
