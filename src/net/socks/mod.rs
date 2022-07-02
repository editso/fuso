use std::marker::PhantomData;

use crate::{
    ext::AsyncReadExt,
    AsyncRead, AsyncWrite
};

pub struct Socks5<S>(PhantomData<S>);

impl<S> Socks5<S> {
    pub fn new() -> Self {
        Socks5(PhantomData)
    }
}