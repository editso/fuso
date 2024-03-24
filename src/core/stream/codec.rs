use std::{
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    core::{
        io::{AsyncRead, AsyncWrite},
        BoxedStream,
    },
    error,
};

pub trait AsyncEncoder {
    fn poll_encode(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        stream: &mut BoxedStream<'_>,
        buf: &[u8],
    ) -> Poll<error::Result<usize>>;
}

pub trait AsyncDecoder {
    fn poll_decode(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        stream: &mut BoxedStream<'_>,
        buf: &mut [u8],
    ) -> Poll<error::Result<usize>>;
}

pub trait Codec: AsyncDecoder + AsyncEncoder {}

pub struct BoxedCodec<'a>(pub(crate) Box<dyn Codec + Send + Unpin + 'a>);

#[pin_project::pin_project]
pub struct PairCodec<'a> {
    #[pin]
    pub(crate) first: BoxedCodec<'a>,
    pub(crate) second: BoxedCodec<'a>,
}

#[pin_project::pin_project]
pub struct CodecStream<S, C> {
    #[pin]
    codec: C,
    stream: S,
}

pub struct EmptyCodec;

impl<T> Codec for T where T: AsyncDecoder + AsyncEncoder + Unpin {}

impl<'a> AsyncEncoder for BoxedCodec<'a> {
    #[inline]
    fn poll_encode(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        stream: &mut BoxedStream<'_>,
        buf: &[u8],
    ) -> Poll<error::Result<usize>> {
        Pin::new(&mut *self.0).poll_encode(cx, stream, buf)
    }
}

impl<'a> AsyncDecoder for BoxedCodec<'a> {
    #[inline]
    fn poll_decode(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        stream: &mut BoxedStream<'_>,
        buf: &mut [u8],
    ) -> Poll<error::Result<usize>> {
        Pin::new(&mut *self.0).poll_decode(cx, stream, buf)
    }
}

impl<'a> AsyncEncoder for PairCodec<'a> {
    #[inline]
    fn poll_encode(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        stream: &mut BoxedStream<'_>,
        buf: &[u8],
    ) -> Poll<error::Result<usize>> {
        let mut this = self.project();

        let mut stream = BoxedStream::new(CodecStream {
            stream,
            codec: &mut *this.first,
        });

        Pin::new(&mut *this.second).poll_encode(cx, &mut stream, buf)
    }
}

impl<'a> AsyncDecoder for PairCodec<'a> {
    #[inline]
    fn poll_decode(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        stream: &mut BoxedStream<'_>,
        buf: &mut [u8],
    ) -> Poll<error::Result<usize>> {
        let mut this = self.project();

        let mut stream = BoxedStream::new(CodecStream {
            stream,
            codec: &mut *this.first,
        });

        Pin::new(&mut *this.second).poll_decode(cx, &mut stream, buf)
    }
}

impl<'s, 'c> AsyncWrite for CodecStream<&mut BoxedStream<'s>, &mut BoxedCodec<'c>> {
    #[inline]
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<error::Result<usize>> {
        let mut this = self.project();
        match Pin::new(&mut **this.codec).poll_encode(cx, &mut this.stream, buf)? {
            Poll::Pending => Poll::Pending,
            Poll::Ready(_) => Poll::Ready(Ok(buf.len())),
        }
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<error::Result<()>> {
        let this = self.project();
        Pin::new(&mut *this.stream).poll_flush(cx)
    }
}

impl<'s, 'c> AsyncRead for CodecStream<&mut BoxedStream<'s>, &mut BoxedCodec<'c>> {
    #[inline]
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<error::Result<usize>> {
        let mut this = self.project();
        Pin::new(&mut **this.codec).poll_decode(cx, &mut *this.stream, buf)
    }
}

impl AsyncEncoder for EmptyCodec {
    #[inline]
    fn poll_encode(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        stream: &mut BoxedStream<'_>,
        buf: &[u8],
    ) -> Poll<error::Result<usize>> {
        Pin::new(stream).poll_write(cx, buf)
    }
}

impl AsyncDecoder for EmptyCodec {
    #[inline]
    fn poll_decode(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        stream: &mut BoxedStream<'_>,
        buf: &mut [u8],
    ) -> Poll<error::Result<usize>> {
        Pin::new(stream).poll_read(cx, buf)
    }
}
