use std::{pin::Pin, task::Poll};

use aes::Aes128;
use block_modes::{block_padding::Pkcs7, BlockMode, Cbc};
use bytes::{Buf, BufMut, BytesMut};
use fuso_core::Cipher;
use futures::{AsyncReadExt, AsyncWriteExt};
use hex::ToHex;
use rand::Rng;
use smol::future::FutureExt;

type Aes128Cbc = Cbc<Aes128, Pkcs7>;

pub struct Aes {
    aes: Aes128Cbc,
    key: [u8; 16],
    iv: [u8; 16],
}


impl Aes {
    pub fn try_with(key: &str) -> fuso_core::Result<Self> {
        let secret = key.as_bytes();
        let mut key_buf = [0; 16];
        let mut iv_buf = [0; 16];

        if secret.len() > 16 {
            Err("bad secret key".into())
        } else {
            key_buf[..secret.len()].copy_from_slice(secret);

            let mut rand = rand::rngs::OsRng;
            rand.fill(&mut key_buf[secret.len()..]);
            rand.fill(&mut iv_buf[..]);

            let aes = Aes128Cbc::new_from_slices(&key_buf, &iv_buf).unwrap();
            Ok(Self {
                aes,
                key: key_buf,
                iv: iv_buf,
            })
        }
    }

    pub fn to_hex_string(&self) -> String {
        let key: String = self.key.encode_hex();
        let iv: String = self.iv.encode_hex();
        format!("{}{}", key, iv)
    }
}


impl TryFrom<String> for Aes {
    type Error = fuso_core::Error;

    fn try_from(secret: String) -> Result<Self, Self::Error> {
        let (key, iv) = hex::decode(secret).map_or_else(
            |e| Err(fuso_core::Error::with_str(e.to_string())),
            |data| {
                if data.len() != 32 {
                    Err("bad ase key".into())
                } else {
                    let mut key = [0; 16];
                    let mut iv = [0; 16];

                    key.copy_from_slice(&data[..16]);
                    iv.copy_from_slice(&data[16..]);

                    Ok((key, iv))
                }
            },
        )?;

        let cbc = Aes128Cbc::new_from_slices(&key, &iv)
            .map_err(|e| fuso_core::Error::with_str(e.to_string()))?;

        Ok(Self { key, iv, aes: cbc })
    }
}

impl Cipher for Aes {
    fn poll_decrypt(
        &mut self,
        mut io: Box<&mut (dyn futures::AsyncRead + Unpin + Send + Sync + 'static)>,
        cx: &mut std::task::Context<'_>,
        _: &mut [u8],
    ) -> std::task::Poll<std::io::Result<Vec<u8>>> {
        let mut buf = BytesMut::new();
        buf.resize(4, 0);

        let n = match Pin::new(&mut io.read_exact(&mut buf)).poll(cx)? {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(_) => buf.get_u32(),
        };

        buf.resize(n as usize, 0);

        match Pin::new(&mut io.read_exact(&mut buf)).poll(cx)? {
            std::task::Poll::Pending => Poll::Pending,
            std::task::Poll::Ready(_) => {
                let cipher = self.aes.clone();

                let data = cipher.decrypt(&mut buf).map_err(|e| {
                    log::warn!("[aes] decrypt error {}", e);
                    fuso_core::Error::with_str(e.to_string())
                })?;

                Poll::Ready(Ok(data.to_vec()))
            }
        }
    }

    fn poll_encrypt(
        &mut self,
        mut io: Box<&mut (dyn futures::AsyncWrite + Unpin + Send + Sync + 'static)>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let encrypt_data = Vec::new();

        let data = buf
            .chunks(4095)
            .into_iter()
            .try_fold(encrypt_data, |mut buffer, data| {
                let mut encrypt_data = [0; 4096];

                let cipher = self.aes.clone();

                encrypt_data[..data.len()].copy_from_slice(data);

                cipher
                    .encrypt(&mut encrypt_data, data.len())
                    .map_or_else(
                        |e| {
                            log::warn!("[aes] encrypt error {}", e);
                            Err(fuso_core::Error::with_str(e.to_string()))
                        },
                        |n| Ok(n.len()),
                    )
                    .map_or_else(
                        |e| Err(e),
                        |pos| {
                            // let _ = Pkcs7::pad_block(&mut encrypt_data, pos);
                            buffer.put_u32(pos as u32);
                            buffer.put_slice(&encrypt_data[..pos]);

                            Ok(buffer)
                        },
                    )
            })?;

        match Pin::new(&mut io).write_all(&data).poll(cx)? {
            Poll::Ready(_) => Poll::Ready(Ok(buf.len())),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[test]
#[allow(unused)]
fn test_try_from() {
    let aes = Aes::try_with("random").unwrap();

    println!("{}", aes.to_hex_string());

    let aes: Aes = String::from("72616e646f6d6ddb05bb4ec531f4a6ae0782c1ec2a33535be00d17a46b83efd1")
        .try_into()
        .unwrap();

    let plaintext = b"GET / HTTP/1.1\r\n124234234434333";
    let cipher = Aes128Cbc::new_from_slices(&aes.key, &aes.iv).unwrap();

    let mut buffer = [0u8; 32];
    // copy message to the buffer
    let pos = plaintext.len();
    buffer[..pos].copy_from_slice(plaintext);
    let ciphertext = cipher.encrypt(&mut buffer, pos).unwrap();

    let mut ciphertext = [
        64, 240, 250, 146, 223, 169, 130, 111, 35, 75, 185, 186, 171, 196, 224, 139, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    let cipher = Aes128Cbc::new_from_slices(&aes.key, &aes.iv).unwrap();
    let mut buf = ciphertext.to_vec();

    // let data = Pkcs7::unpad(&mut ciphertext).unwrap();

    // println!("{:?}", data);

    // let decrypted_ciphertext = cipher.decrypt(&mut buf[..data.len()]).unwrap();
}

#[test]
fn test_aes() {
    use aes::Aes128;
    use block_modes::block_padding::Pkcs7;
    use block_modes::{BlockMode, Cbc};
    use hex_literal::hex;

    // create an alias for convenience
    type Aes128Cbc = Cbc<Aes128, Pkcs7>;

    let key = hex!("000102030405060708090a0b0c0d0e0f");
    let iv = hex!("f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff");
    let plaintext = b"GET / HTTP/1.1\r\nHost: 127.0.0.1:";
    let cipher = Aes128Cbc::new_from_slices(&key, &iv).unwrap();

    // buffer must have enough space for message+padding
    let mut buffer = [0u8; 32];
    // copy message to the buffer
    let pos = plaintext.len();
    buffer[..pos].copy_from_slice(plaintext);
    let ciphertext = cipher.encrypt(&mut buffer, pos).unwrap();

    assert_eq!(ciphertext, hex!("1b7a4c403124ae2fb52bedc534d82fa8"));

    // re-create cipher mode instance and decrypt the message
    let cipher = Aes128Cbc::new_from_slices(&key, &iv).unwrap();
    let mut buf = ciphertext.to_vec();
    let decrypted_ciphertext = cipher.decrypt(&mut buf).unwrap();

    assert_eq!(decrypted_ciphertext, plaintext);
}
