mod lz4;

use std::pin::Pin;

use crate::core::stream::codec::{EmptyCodec, PairCodec};
use crate::{config::Compress, error};

use crate::core::{
    io::{AsyncRead, AsyncWrite},
    BoxedStream,
};

use super::codec::{AsyncDecoder, AsyncEncoder, BoxedCodec};

#[pin_project::pin_project]
pub struct CompressedStream<'a> {
    #[pin]
    stream: BoxedStream<'a>,
    compressor: BoxedCodec<'a>,
}

pub async fn compress_stream<'a, S>(
    stream: S,
    compress: Vec<Compress>,
) -> error::Result<CompressedStream<'a>>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'a,
{
    fn use_compress<'a>(compress: &Compress) -> BoxedCodec<'a> {
        match compress {
            Compress::Lz4 => BoxedCodec(Box::new(lz4::Lz4Compressor {})),
        }
    }

    let compressor = if compress.is_empty() {
        BoxedCodec(Box::new(EmptyCodec))
    } else if compress.len() == 1 {
        use_compress(&compress[0])
    } else {
        let mut compressor = BoxedCodec(Box::new({
            PairCodec {
                first: use_compress(&compress[0]),
                second: use_compress(&compress[1]),
            }
        }));

        for crypto_type in &compress[2..] {
            compressor = BoxedCodec(Box::new(PairCodec {
                first: compressor,
                second: use_compress(crypto_type),
            }))
        }

        compressor
    };

    Ok(CompressedStream {
        stream: BoxedStream::new(stream),
        compressor,
    })
}

impl<'a> AsyncRead for CompressedStream<'a> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<error::Result<usize>> {
        let mut this = self.project();
        Pin::new(&mut *this.compressor).poll_decode(cx, &mut *this.stream, buf)
    }
}


impl<'a> AsyncWrite for CompressedStream<'a> {
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<error::Result<()>> {
        let mut this = self.project();
        Pin::new(&mut *this.stream).poll_flush(cx)
    }

    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<error::Result<usize>> {
        let mut this = self.project();
        Pin::new(&mut *this.compressor).poll_encode(cx, &mut this.stream, buf)
    }
}
