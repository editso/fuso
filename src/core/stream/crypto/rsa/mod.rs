use std::pin::Pin;

use crate::core::io::{AsyncRead, AsyncWrite};

use super::{AsyncDecrypt, AsyncEncrypt};

pub struct RsaCrypto {}

impl AsyncEncrypt for RsaCrypto {
    fn poll_encrypt(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        stream: &mut crate::core::BoxedStream<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        Pin::new(stream).poll_write(cx, buf)
    }
}

impl AsyncDecrypt for RsaCrypto {
    fn poll_decrypt(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        stream: &mut crate::core::BoxedStream<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        Pin::new(stream).poll_read(cx, buf)
    }
}
