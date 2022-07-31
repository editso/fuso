use std::pin::Pin;

use rsa::{
    pkcs1::EncodeRsaPublicKey,
    pkcs8::{DecodePublicKey, EncodePublicKey, LineEnding},
};

use crate::{
    compress::Lz4Compress,
    encryption::{AESEncryptor, RSAEncryptor},
    ext::{AsyncReadExt, AsyncWriteExt},
    protocol::AsyncRecvPacket,
    Address, DecorateProvider, FusoStream, Provider, Stream, ToBoxStream,
};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub enum PenetrateHandshake {
    Server,
    Client,
}

pub struct PenetrateDecorator;

impl PenetrateHandshake {
    pub fn server_handshake<S>(
        mut client: S,
    ) -> BoxedFuture<(FusoStream, Option<DecorateProvider<FusoStream>>)>
    where
        S: Stream + Unpin + Send + 'static,
    {
        Box::pin(async move {
            let mut buf = [0u8; 4];
            client.read_exact(&mut buf).await?;
            let len = u32::from_be_bytes(buf) as usize;

            let mut buf = Vec::with_capacity(len);

            unsafe { buf.set_len(len) }

            client.read_exact(&mut buf).await?;

            let priv_key = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 1024)?;
            let publ_key = rsa::RsaPublicKey::from(&priv_key);
            let client_publ_key = rsa::RsaPublicKey::from_public_key_der(&buf)?;

            let pem = publ_key.to_public_key_der()?;
            let pem = pem.as_ref();
            let len = pem.len() as u32;

            client.write_all(&len.to_be_bytes()).await?;
            client.write_all(pem).await?;

            let encryptor =
                RSAEncryptor::new(client, client_publ_key, priv_key).into_boxed_stream();

            Ok((encryptor, Some(DecorateProvider::wrap(PenetrateDecorator))))
        })
    }

    pub fn client_handshake<S>(
        mut stream: S,
    ) -> BoxedFuture<(FusoStream, Option<DecorateProvider<FusoStream>>)>
    where
        S: Stream + Unpin + Send + 'static,
    {
        Box::pin(async move {
            let priv_key = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 1024)?;
            let publ_key = rsa::RsaPublicKey::from(&priv_key);

            let pem = publ_key.to_public_key_der()?;
            let pem = pem.as_ref();

            let len = pem.len() as u32;

            stream.write_all(&len.to_be_bytes()).await?;
            stream.write_all(pem).await?;

            let mut buf = [0u8; 4];
            stream.read_exact(&mut buf).await?;
            let len = u32::from_be_bytes(buf) as usize;

            let mut buf = Vec::with_capacity(len);

            unsafe {
                buf.set_len(len);
            }

            stream.read_exact(&mut buf).await?;

            let server_publ_key = rsa::RsaPublicKey::from_public_key_der(&buf)?;

            let encryptor =
                RSAEncryptor::new(stream, server_publ_key, priv_key).into_boxed_stream();

            Ok((encryptor, Some(DecorateProvider::wrap(PenetrateDecorator))))
        })
    }
}

impl<S> Provider<S> for PenetrateHandshake
where
    S: Stream + Unpin + Send + 'static,
{
    type Output = BoxedFuture<(FusoStream, Option<DecorateProvider<FusoStream>>)>;

    fn call(&self, client: S) -> Self::Output {
        match self {
            PenetrateHandshake::Server => Self::server_handshake(client),
            PenetrateHandshake::Client => Self::client_handshake(client),
        }
    }
}

impl<S> Provider<S> for PenetrateDecorator
where
    S: Stream + Unpin + Send + 'static,
{
    type Output = BoxedFuture<FusoStream>;
    fn call(&self, stream: S) -> Self::Output {
        Box::pin(async move { Ok(Lz4Compress::new(stream).into_boxed_stream()) })
    }
}
