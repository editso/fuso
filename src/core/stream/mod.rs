use self::{compress::CompressedStream, crypto::EncryptedStream};

use super::io::{AsyncRead, AsyncWrite};

pub mod codec;
pub mod compress;
pub mod crypto;
pub mod fallback;
pub mod handshake;

pub trait UseCrypto<'a>: AsyncRead + AsyncWrite + Send + Unpin {
    fn use_crypto(self, cryptos: Vec<crate::config::Crypto>) -> EncryptedStream<'a>
    where
        Self: Sized + 'a,
    {
        crypto::encrypt_stream(self, cryptos)
    }
}

pub trait UseCompress<'a>: AsyncRead + AsyncWrite + Send + Unpin {
    fn use_compress(self, compress: Vec<crate::config::Compress>) -> CompressedStream<'a>
    where
        Self: Sized + 'a,
    {
        compress::compress_stream(self, compress)
    }
}

impl<'a, T> UseCrypto<'a> for T where T: AsyncRead + AsyncWrite + Send + Unpin {}
impl<'a, T> UseCompress<'a> for T where T: AsyncRead + AsyncWrite + Send + Unpin {}
