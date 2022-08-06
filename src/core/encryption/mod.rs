mod aes;
mod rsa;

pub use crate::core::encryption::{aes::AESEncryptor, rsa::RSAEncryptor};

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use crate::ReadBuf;

pub trait Decrypt {
    fn poll_decrypt_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<crate::Result<usize>>;
}

pub trait Encrypt {
    fn poll_encrypt_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<crate::Result<usize>>;
}
