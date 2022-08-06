use std::pin::Pin;

use rsa::pkcs8::{DecodePublicKey, EncodePublicKey};

use crate::{
    compress::Lz4Compress,
    encryption::{AESEncryptor, RSAEncryptor},
    ext::{AsyncReadExt, AsyncWriteExt},
    DecorateProvider, FusoStream, Provider, Stream, ToBoxStream,
};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub enum PenetrateRsaAndAesHandshake {
    Server,
    Client,
}

pub struct PenetrateAesAndLz4Decorator {
    iv: [u8; 16],
    key: [u8; 16],
}

impl PenetrateRsaAndAesHandshake {
    pub fn server_handshake<S>(
        client: S,
    ) -> BoxedFuture<(FusoStream, Option<DecorateProvider<FusoStream>>)>
    where
        S: Stream + Unpin + Send + 'static,
    {
        Box::pin(async move {
            let mut client = Lz4Compress::new(client);
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

            let mut fuso_stream = RSAEncryptor::new(client, client_publ_key, priv_key);

            let mut iv = [0u8; 16];
            let mut key = [0u8; 16];

            fuso_stream.read_exact(&mut iv).await?;

            fuso_stream.read_exact(&mut key).await?;

            log::trace!("iv: {:?}, key: {:?}", iv, key);

            Ok((
                fuso_stream.into_boxed_stream(),
                Some(DecorateProvider::wrap(PenetrateAesAndLz4Decorator {
                    iv,
                    key,
                })),
            ))
        })
    }

    pub fn client_handshake<S>(
        stream: S,
    ) -> BoxedFuture<(FusoStream, Option<DecorateProvider<FusoStream>>)>
    where
        S: Stream + Unpin + Send + 'static,
    {
        Box::pin(async move {
            let mut stream = Lz4Compress::new(stream);
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

            let mut fuso_stream = RSAEncryptor::new(stream, server_publ_key, priv_key);

            let mut iv = [0u8; 16];
            let mut key = [0u8; 16];

            iv.fill_with(rand::random);
            key.fill_with(rand::random);

            log::trace!("iv: {:?}, key: {:?}", iv, key);

            fuso_stream.write_all(&iv).await?;
            fuso_stream.write_all(&key).await?;

            Ok((
                fuso_stream.into_boxed_stream(),
                Some(DecorateProvider::wrap(PenetrateAesAndLz4Decorator {
                    iv,
                    key,
                })),
            ))
        })
    }
}

impl<S> Provider<S> for PenetrateRsaAndAesHandshake
where
    S: Stream + Unpin + Send + 'static,
{
    type Output = BoxedFuture<(FusoStream, Option<DecorateProvider<FusoStream>>)>;

    fn call(&self, client: S) -> Self::Output {
        match self {
            PenetrateRsaAndAesHandshake::Server => Self::server_handshake(client),
            PenetrateRsaAndAesHandshake::Client => Self::client_handshake(client),
        }
    }
}

impl<S> Provider<S> for PenetrateAesAndLz4Decorator
where
    S: Stream + Unpin + Send + 'static,
{
    type Output = BoxedFuture<FusoStream>;
    fn call(&self, stream: S) -> Self::Output {
        let iv = self.iv.clone();
        let key = self.key.clone();
        Box::pin(async move {
            let lz4 = Lz4Compress::new(stream);
            let aes = AESEncryptor::new(lz4, iv, key);
            Ok(aes.into_boxed_stream())
        })
    }
}
