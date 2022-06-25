pub mod xor;

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use crate::{AsyncRead, AsyncWrite, Result};

pub trait Encrypt {
    fn poll_crypt(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<Vec<u8>>>;
}

pub trait Decrypt {
    fn poll_decrypt(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<Vec<u8>>>;
}

pub trait Encryption {
    type Stream: AsyncRead + AsyncWrite + Unpin + 'static;
    type Cipher: Encrypt + Decrypt + 'static;
    fn poll_encryption(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        stream: &mut Self::Stream,
    ) -> Poll<Result<Self::Cipher>>;
}
