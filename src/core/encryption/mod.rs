use crate::{server::server::Server, AsyncRead, AsyncWrite, ReadBuf};

pub struct Encryption<S> {
    pub(crate) factory: S,
    pub(crate) ciphers: Vec<usize>,
}

impl<S> Encryption<S> {
    pub fn cipher(self) -> Self {
        unimplemented!()
    }
}
