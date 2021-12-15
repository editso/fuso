use std::{pin::Pin, task::Poll};

use fuso_core::Cipher;
use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use rsa::{PaddingScheme, PublicKey, RsaPrivateKey, RsaPublicKey};
use smol::future::FutureExt;

pub struct Rsa {
    public_key: RsaPublicKey,
    private_key: RsaPrivateKey,
}

impl Rsa {
    pub fn new(public_key: RsaPublicKey, private_key: RsaPrivateKey) -> Self {
        Self {
            public_key,
            private_key,
        }
    }
}

impl Cipher for Rsa {
    fn poll_decrypt(
        &mut self,
        mut io: Box<&mut (dyn AsyncRead + Unpin + Send + Sync + 'static)>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<Vec<u8>>> {
        let mut packet = Vec::new();

        packet.resize(256, 0);

        match Box::pin(Pin::new(&mut io).read_exact(&mut packet)).poll(cx)? {
            Poll::Pending => Poll::Pending,
            Poll::Ready(_) => {
                let padding = PaddingScheme::new_pkcs1v15_encrypt();
                let data = self.private_key.decrypt(padding, &packet).map_err(|e| {
                    log::warn!("[rsa] {}", e);
                    fuso_core::Error::with_str(e.to_string())
                })?;
                Poll::Ready(Ok(data))
            }
        }
    }

    fn poll_encrypt(
        &mut self,
        mut io: Box<&mut (dyn AsyncWrite + Unpin + Send + Sync + 'static)>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        use rand::rngs::OsRng;
        let mut rng = OsRng;
        let padding = PaddingScheme::new_pkcs1v15_encrypt();

        let data = self
            .public_key
            .encrypt(&mut rng, padding, buf)
            .map_err(|e| {
                log::warn!("[rsa] {}", e);
                fuso_core::Error::with_str(e.to_string())
            })?;

        match Box::pin(Pin::new(&mut io).write_all(&data)).poll(cx)? {
            Poll::Pending => Poll::Pending,
            Poll::Ready(_) => Poll::Ready(Ok(buf.len())),
        }
    }
}


pub mod server {
    use async_trait::async_trait;
    use bytes::{Buf, BufMut, BytesMut};

    use fuso_core::{Advice, DynCipher};
    use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
    use rsa::{
        pkcs1::{FromRsaPublicKey, ToRsaPublicKey},
        RsaPrivateKey, RsaPublicKey,
    };

    use crate::rsa::Rsa;

    pub struct RsaAdvice;

    #[async_trait]
    impl<T> Advice<T, Box<DynCipher>> for RsaAdvice
    where
        T: AsyncRead + AsyncWrite + Send + Sync + Unpin + 'static,
    {
        async fn advice(&self, io: &mut T) -> fuso_core::Result<Option<Box<DynCipher>>> {
            let mut buf = BytesMut::new();

            buf.resize(5, 0);

            io.read_exact(&mut buf).await?;

            let magic = buf.get_u8();

            if magic != 0x11 {
                return Ok(None);
            }

            let len = buf.get_u32();

            buf.resize(len as usize, 0);

            io.read_exact(&mut buf).await?;

            let pem = String::from_utf8_lossy(&buf);

            let public_key = RsaPublicKey::from_pkcs1_pem(&pem)
                .map_err(|e| fuso_core::Error::with_str(e.to_string()))?;

            log::info!("rsa public key: \n{}", String::from_utf8_lossy(&buf));

            let mut rng = rand::rngs::OsRng;

            let private_key = RsaPrivateKey::new(&mut rng, 2048)
                .map_err(|e| fuso_core::Error::with_str(e.to_string()))?;

            let my_public_key = RsaPublicKey::from(&private_key);

            let pem = my_public_key
                .to_pkcs1_pem()
                .map_err(|e| fuso_core::Error::with_str(e.to_string()))?;

            let pem = pem.as_bytes();
            let mut buf = BytesMut::new();

            buf.put_u8(0x12);
            buf.put_u32(pem.len() as u32);
            buf.put_slice(&pem);

            io.write_all(&buf).await?;

            Ok(Some(Box::new(Rsa::new(public_key, private_key))))
        }
    }
}

pub mod client {
    use async_trait::async_trait;
    use bytes::{Buf, BufMut, BytesMut};

    use fuso_core::{Advice, DynCipher};
    use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
    use rsa::{
        pkcs1::{FromRsaPublicKey, ToRsaPublicKey},
        RsaPrivateKey, RsaPublicKey,
    };

    use super::Rsa;

    pub struct RsaAdvice;

    #[async_trait]
    impl<T> Advice<T, Box<DynCipher>> for RsaAdvice
    where
        T: AsyncRead + AsyncWrite + Send + Sync + Unpin + 'static,
    {
        async fn advice(&self, io: &mut T) -> fuso_core::Result<Option<Box<DynCipher>>> {
            let mut rng = rand::rngs::OsRng;

            let private_key = RsaPrivateKey::new(&mut rng, 2048)
                .map_err(|e| fuso_core::Error::with_str(e.to_string()))?;

            let public_key = RsaPublicKey::from(&private_key);

            let pem = public_key
                .to_pkcs1_pem()
                .map_err(|e| fuso_core::Error::with_str(e.to_string()))?;

            let pem = pem.as_bytes();

            let mut buf = BytesMut::new();

            buf.put_u8(0x11);
            buf.put_u32(pem.len() as u32);
            buf.put_slice(&pem);

            io.write_all(&buf).await?;

            buf.resize(5, 0);

            io.read_exact(&mut buf).await?;

            let code = buf.get_u8();

            if code != 0x12 {
                return Ok(None);
            }

            let len = buf.get_u32() as usize;

            buf.resize(len, 0);
            io.read_exact(&mut buf).await?;

            let pem = String::from_utf8_lossy(&buf);

            let public_key = RsaPublicKey::from_pkcs1_pem(&pem)
                .map_err(|e| fuso_core::Error::with_str(e.to_string()))?;

            log::info!("rsa public key: \n{}", String::from_utf8_lossy(&buf));

            let rsa = Rsa::new(public_key, private_key);

            Ok(Some(Box::new(rsa)))
        }
    }
}

#[test]
fn test_rsa() {
    use rand::rngs::OsRng;
    use rsa::{PaddingScheme, PublicKey, RsaPrivateKey, RsaPublicKey};
    let mut rng = OsRng;
    let bits = 2048;
    let private_key_1 = RsaPrivateKey::new(&mut rng, bits).expect("failed to generate a key");
    let public_key_1 = RsaPublicKey::from(&private_key_1);

    // public_key.to_pkcs1_pem();

    // Encrypt
    let data = b"hello world";
    let padding = PaddingScheme::new_pkcs1v15_encrypt();
    let enc_data = public_key_1
        .encrypt(&mut rng, padding, &data[..])
        .expect("failed to encrypt");
    assert_ne!(&data[..], &enc_data[..]);

    println!("{:?} len = {}", enc_data, enc_data.len());

    // Decrypt
    let padding = PaddingScheme::new_pkcs1v15_encrypt();

    let dec_data = private_key_1
        .decrypt(padding, &enc_data)
        .expect("failed to decrypt");
}
