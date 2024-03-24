use std::pin::Pin;

use crate::core::{
    io::{AsyncRead, AsyncWrite},
    stream::codec::{AsyncDecoder, AsyncEncoder},
};

pub struct Lz4Compressor {}

impl AsyncEncoder for Lz4Compressor {
    fn poll_encode(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        stream: &mut crate::core::BoxedStream<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        Pin::new(stream).poll_write(cx, buf)
    }
}

impl AsyncDecoder for Lz4Compressor {
    fn poll_decode(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        stream: &mut crate::core::BoxedStream<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        Pin::new(stream).poll_read(cx, buf)
    }
}
