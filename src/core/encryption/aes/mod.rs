use std::{pin::Pin, task::Poll};

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};

use crate::{guard::buffer::Buffer, AsyncRead, AsyncWrite, NetSocket, ReadBuf};

use super::{Decrypt, Encrypt};

// ref https://docs.rs/cbc/0.1.2/cbc/

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

pub struct AESEncryptor<T> {
    target: T,
    iv: [u8; 16],
    key: [u8; 16],
    aes_ebuf: Option<Vec<u8>>,
    aes_dbuf: Buffer<u8>,
    aes_rbuf: Option<Vec<u8>>,
    aes_epos: usize,
    aes_dpos: usize,
    aes_rpos: usize,
    aes_dinit: bool,
}

impl<T> NetSocket for AESEncryptor<T>
where
    T: NetSocket,
{
    fn peer_addr(&self) -> crate::Result<crate::Address> {
        self.target.peer_addr()
    }

    fn local_addr(&self) -> crate::Result<crate::Address> {
        self.target.local_addr()
    }
}

impl<T> AsyncRead for AESEncryptor<T>
where
    T: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> std::task::Poll<crate::Result<usize>> {
        if !self.aes_dbuf.is_empty() {
            let n = self.aes_dbuf.read_to_buffer(buf.initialize_unfilled());
            buf.advance(n);
            return Poll::Ready(Ok(n));
        } else {
            self.poll_decrypt_read(cx, buf)
        }
    }
}

impl<T> AsyncWrite for AESEncryptor<T>
where
    T: AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::Result<usize>> {
        if let Some(ebuf) = self.aes_ebuf.take() {
            loop {
                let epos = self.aes_epos;
                match Pin::new(&mut self.target).poll_write(cx, &ebuf[epos..])? {
                    Poll::Ready(0) => break Poll::Ready(Ok(0)),
                    Poll::Ready(n) => {
                        self.aes_epos += n;
                        if epos == ebuf.len() {
                            break Poll::Ready(Ok(buf.len()));
                        }
                    }
                    Poll::Pending => {
                        drop(std::mem::replace(&mut self.aes_ebuf, Some(ebuf)));
                        break Poll::Pending;
                    }
                }
            }
        } else {
            self.poll_encrypt_write(cx, buf)
        }
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        Pin::new(&mut self.target).poll_flush(cx)
    }

    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        Pin::new(&mut self.target).poll_close(cx)
    }
}

impl<T> Encrypt for AESEncryptor<T>
where
    T: AsyncWrite + Unpin,
{
    fn poll_encrypt_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::Result<usize>> {
        let mut encrypted_buf = unsafe {
            let mut buf = Vec::with_capacity(buf.len() + 16 + 4);
            buf.set_len(buf.len() + 16 + 4);
            buf
        };

        encrypted_buf[4..].copy_from_slice(buf);

        let encrypted_len = {
            let encrypted = Aes128CbcEnc::new_from_slices(&self.key, &self.iv)?
                .encrypt_padded_mut::<Pkcs7>(&mut encrypted_buf[4..], buf.len())?;
            encrypted.len()
        };

        encrypted_buf[..4].clone_from_slice(&encrypted_len.to_le_bytes());

        let mut epos = 0;

        loop {
            match Pin::new(&mut self.target).poll_write(cx, &encrypted_buf)? {
                Poll::Ready(0) => break Poll::Ready(Ok(0)),
                Poll::Ready(n) => {
                    epos += n;
                    if epos == encrypted_buf.len() {
                        break Poll::Ready(Ok(buf.len()));
                    }
                }
                Poll::Pending => {
                    drop(std::mem::replace(
                        &mut self.aes_ebuf,
                        Some(encrypted_buf[epos..].to_vec()),
                    ));
                    self.aes_epos = 0;
                    break Poll::Pending;
                }
            }
        }
    }
}

impl<T> Decrypt for AESEncryptor<T>
where
    T: AsyncRead + Unpin,
{
    fn poll_decrypt_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> std::task::Poll<crate::Result<usize>> {
        let rbuf = self.aes_rbuf.take();
        let mut rbuf = rbuf.unwrap_or_else(|| {
            let mut buf = Vec::new();
            buf.resize(4, 0);
            buf
        });

        loop {
            let rpos = self.aes_rpos;

            if !self.aes_dinit && rpos == rbuf.len() {
                let len = unsafe { *(rbuf.as_ptr() as *const u32) } as usize;
                rbuf = unsafe {
                    let data_buf = Vec::with_capacity(len);
                    rbuf.set_len(len);
                    data_buf
                };
                self.aes_dinit = true;
            } else if self.aes_dinit && rpos == rbuf.len() {
                self.aes_dinit = false;
                let rem = buf.remaining();
                let decrypted = Aes128CbcDec::new_from_slices(&self.key, &self.iv)?
                    .decrypt_padded_mut::<Pkcs7>(&mut rbuf)?;
                if rem > decrypted.len() {
                    unsafe {
                        let unfilled = buf.initialize_unfilled();
                        std::ptr::copy(decrypted.as_ptr(), unfilled.as_mut_ptr(), decrypted.len());
                        return Poll::Ready(Ok(decrypted.len()));
                    }
                } else {
                    unsafe {
                        let unfilled = buf.initialize_unfilled();
                        let unfilled_len = unfilled.len();
                        std::ptr::copy(decrypted.as_ptr(), unfilled.as_mut_ptr(), unfilled_len);
                        self.aes_dbuf.push_back(&decrypted[unfilled_len..]);
                        return Poll::Ready(Ok(unfilled_len));
                    }
                }
            }

            let mut read_buf = ReadBuf::new(&mut rbuf[rpos..]);
            match Pin::new(&mut self.target).poll_read(cx, &mut read_buf)? {
                Poll::Ready(0) => return Poll::Ready(Ok(0)),
                Poll::Ready(n) => {
                    self.aes_dpos += n;
                }
                Poll::Pending => {
                    drop(std::mem::replace(&mut self.aes_rbuf, Some(rbuf)));
                    return Poll::Pending;
                }
            }
        }
    }
}
