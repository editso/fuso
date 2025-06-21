mod aes;
mod rsa;

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use crate::core::{
    io::{AsyncRead, AsyncWrite},
    stream::codec::{EmptyCodec, PairCodec},
};
use crate::{config::Crypto, core::AbstractStream, error};

use super::codec::{AsyncDecoder, AsyncEncoder, AbstractCodec};

pub trait AsyncEncrypt {
    fn poll_encrypt(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        stream: &mut AbstractStream<'_>,
        buf: &[u8],
    ) -> Poll<error::Result<usize>>;
}

pub trait AsyncDecrypt {
    fn poll_decrypt(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        stream: &mut AbstractStream<'_>,
        buf: &mut [u8],
    ) -> Poll<error::Result<usize>>;
}

pub trait AsyncCrypto: AsyncEncrypt + AsyncDecrypt {}

#[pin_project::pin_project]
pub struct EncryptedStream<'a> {
    #[pin]
    stream: AbstractStream<'a>,
    crypto: AbstractCodec<'a>,
}

pub fn encrypt_stream<'a, 'crypto, C, S>(stream: S, mut cryptos: C) -> EncryptedStream<'a>
where
    C: Iterator<Item = &'crypto Crypto>,
    S: AsyncRead + AsyncWrite + Send + Unpin + 'a,
{
    fn use_crypto<'a>(crypto: &Crypto) -> AbstractCodec<'a> {
        match crypto {
            Crypto::Rsa => AbstractCodec(Box::new(rsa::RsaCrypto {})),
            Crypto::Aes => AbstractCodec(Box::new(aes::AesCrypto {})),
        }
    }

    let mut crypto = match cryptos.next() {
        None => AbstractCodec(Box::new(EmptyCodec)),
        Some(crypto) => match cryptos.next() {
            None => use_crypto(&crypto),
            Some(next) => AbstractCodec(Box::new({
                PairCodec {
                    first: use_crypto(&crypto),
                    second: use_crypto(&next),
                }
            })),
        },
    };

    for crypto_type in cryptos {
        crypto = AbstractCodec(Box::new(PairCodec {
            first: crypto,
            second: use_crypto(&crypto_type),
        }))
    }

    EncryptedStream {
        crypto,
        stream: AbstractStream::new(stream),
    }
}

impl<T> AsyncCrypto for T where T: AsyncDecrypt + AsyncEncrypt {}

impl<T> AsyncDecoder for T
where
    T: AsyncDecrypt + Unpin,
{
    fn poll_decode(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        stream: &mut AbstractStream<'_>,
        buf: &mut [u8],
    ) -> Poll<error::Result<usize>> {
        Pin::new(&mut *self).poll_decrypt(cx, stream, buf)
    }
}

impl<T> AsyncEncoder for T
where
    T: AsyncEncrypt + Unpin,
{
    fn poll_encode(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        stream: &mut AbstractStream<'_>,
        buf: &[u8],
    ) -> Poll<error::Result<usize>> {
        Pin::new(&mut *self).poll_encrypt(cx, stream, buf)
    }
}

impl<'a> AsyncRead for EncryptedStream<'a> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<error::Result<usize>> {
        let mut this = self.project();
        Pin::new(&mut *this.crypto).poll_decode(cx, &mut *this.stream, buf)
    }
}

impl<'a> AsyncWrite for EncryptedStream<'a> {
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<error::Result<()>> {
        self.project().stream.poll_flush(cx)
    }

    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<error::Result<usize>> {
        let mut this = self.project();
        Pin::new(&mut *this.crypto).poll_encode(cx, &mut *this.stream, buf)
    }
}
