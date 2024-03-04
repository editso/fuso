use std::pin::Pin;

use crate::{
    config::Crypto,
    core::{
        io::AsyncReadExt,
        stream::{compress, crypto},
    },
    error,
};

use super::{
    io::{AsyncRead, AsyncWrite},
    BoxedFuture, BoxedStream,
};

pub trait Handshake<'l> {
    type C;
    type Output;
    fn do_handshake(self, conf: &'l Self::C) -> Self::Output;
}

impl<'l, T> Handshake<'l> for T
where
    T: AsyncWrite + AsyncRead + Unpin + Send + 'l,
{
    type C = ();

    type Output = BoxedFuture<'l, error::Result<BoxedStream<'l>>>;

    fn do_handshake(self, conf: &'l Self::C) -> Self::Output {
        Box::pin(async move {
            let stream = crypto::encrypt_stream(
                compress::compress_stream(
                    self,
                    vec![crate::config::Compress::Lz4, crate::config::Compress::Lz4],
                )
                .await?,
                vec![crate::config::Crypto::Aes, crate::config::Crypto::Rsa],
            )
            .await?;

            Ok(BoxedStream::new(stream))
        })
    }
}
