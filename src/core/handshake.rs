use super::BoxedFuture;

pub trait Handshaker<T> {
    fn do_handshake<'a>(&'a mut self, stream: &'a mut T) -> BoxedFuture<'a, crate::error::Result<()>>;
}


