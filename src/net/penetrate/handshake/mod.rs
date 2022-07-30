use std::pin::Pin;

use rsa::{pkcs1::EncodeRsaPublicKey, pkcs8::LineEnding};

use crate::{
    encryption::{AESEncryptor, RSAEncryptor},
    ext::{AsyncReadExt, AsyncWriteExt},
    protocol::AsyncRecvPacket,
    Address, DecorateProvider, FusoStream, Provider, Stream, ToBoxStream,
};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;
type Transporter<S> = DecorateProvider<S>;

pub struct PenetrateHandshake;
pub struct PenetrateTransporter(Address);

impl<S> Provider<S> for PenetrateHandshake
where
    S: Stream + Unpin + Send + 'static,
{
    type Output = BoxedFuture<(FusoStream, Option<Transporter<FusoStream>>)>;

    fn call(&self, mut client: S) -> Self::Output {
        Box::pin(async move {
            let mut buf = [0u8; 4];
            client.read_exact(&mut buf).await?;
            let len = u32::from_be_bytes(buf) as usize;
            let mut buf = Vec::with_capacity(len);

            unsafe { buf.set_len(len) }

            client.read_exact(&mut buf).await?;

            let priv_key = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 1024)?;
            let publ_key = rsa::RsaPublicKey::from(&priv_key);

            let pem = publ_key.to_pkcs1_pem(LineEnding::CRLF)?;
            let pem = pem.as_bytes();
            let len = pem.len() as u32;

            client.write_all(&len.to_be_bytes()).await?;
            client.write_all(pem).await?;
            let client_address = client.peer_addr()?;

            let encryptor = RSAEncryptor::new(client, publ_key, priv_key).into_boxed_stream();

            Ok((
                encryptor,
                Some(Transporter::wrap(PenetrateTransporter(client_address))),
            ))
        })
    }
}

impl<S> Provider<S> for PenetrateTransporter
where
    S: Stream + Unpin + Send + 'static,
{
    type Output = BoxedFuture<FusoStream>;
    fn call(&self, mut stream: S) -> Self::Output {
        Box::pin(async move { Ok(stream.into_boxed_stream()) })
    }
}
