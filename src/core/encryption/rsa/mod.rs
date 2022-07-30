use std::{pin::Pin, task::Poll};

use rsa::{PaddingScheme, PublicKey, RsaPrivateKey, RsaPublicKey};

use crate::{guard::buffer::Buffer, AsyncRead, AsyncWrite, NetSocket, ReadBuf};

use super::{Decrypt, Encrypt};

pub struct RSAEncryptor<T> {
    target: T,
    cache: Buffer<u8>,
    rbuf: Option<Vec<u8>>,
    wbuf: Option<Vec<u8>>,
    wpos: usize,
    rpos: usize,
    dinit: bool,
    rsa_priv: RsaPrivateKey,
    rsa_publ: RsaPublicKey,
}

impl<T> RSAEncryptor<T> {
    pub fn new(target: T, publ_key: RsaPublicKey, priv_key: RsaPrivateKey) -> Self {
        Self {
            target,
            rsa_priv: priv_key,
            rsa_publ: publ_key,
            cache: Default::default(),
            rbuf: Default::default(),
            wbuf: Default::default(),
            wpos: Default::default(),
            rpos: Default::default(),
            dinit: Default::default(),
        }
    }
}

impl<T> NetSocket for RSAEncryptor<T>
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

impl<T> AsyncRead for RSAEncryptor<T>
where
    T: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> std::task::Poll<crate::Result<usize>> {
        if !self.cache.is_empty() {
            let unfilled = buf.initialize_unfilled();
            let len = self.cache.read_to_buffer(unfilled);
            buf.advance(len);
            return Poll::Ready(Ok(len));
        } else {
            self.poll_decrypt_read(cx, buf)
        }
    }
}

impl<T> AsyncWrite for RSAEncryptor<T>
where
    T: AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::Result<usize>> {
        if let Some(wbuf) = self.wbuf.take() {
            loop {
                let pos = self.wpos;
                match Pin::new(&mut self.target).poll_write(cx, &wbuf[pos..])? {
                    Poll::Ready(0) => break Poll::Ready(Ok(0)),
                    Poll::Ready(n) => {
                        self.wpos += n;
                        if self.wpos == wbuf.len() {
                            break Poll::Ready(Ok(buf.len()));
                        }
                    }
                    Poll::Pending => {
                        drop(std::mem::replace(&mut self.wbuf, Some(wbuf)));
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

impl<T> Encrypt for RSAEncryptor<T>
where
    T: AsyncWrite + Unpin,
{
    fn poll_encrypt_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::Result<usize>> {
        let mut rng = rand::thread_rng();
        let ps = PaddingScheme::new_pkcs1v15_encrypt();
        let encrypted_data = self.rsa_publ.encrypt(&mut rng, ps, buf)?;
        let encrypted_len = encrypted_data.len() as u32;
        let mut encrypted_buf = Vec::new();

        encrypted_buf.extend(&encrypted_len.to_le_bytes());
        encrypted_buf.extend_from_slice(&encrypted_data);

        let mut pos = 0;

        loop {
            match Pin::new(&mut self.target).poll_write(cx, &encrypted_buf[pos..])? {
                Poll::Ready(0) => return Poll::Ready(Ok(0)),
                std::task::Poll::Ready(n) => {
                    pos += n;
                    if pos == encrypted_buf.len() {
                        break Poll::Ready(Ok(buf.len()));
                    }
                }
                std::task::Poll::Pending => {
                    self.wpos = 0;
                    drop(std::mem::replace(
                        &mut self.wbuf,
                        Some(encrypted_buf[pos..].to_vec()),
                    ));
                    break Poll::Pending;
                }
            }
        }
    }
}

impl<T> Decrypt for RSAEncryptor<T>
where
    T: AsyncRead + Unpin,
{
    fn poll_decrypt_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> std::task::Poll<crate::Result<usize>> {
        let mut rbuf = self.rbuf.take().unwrap_or_else(|| {
            let mut buf = Vec::new();
            buf.resize(4, 0);
            buf
        });

        loop {
            let rpos = self.rpos;

            if rpos == rbuf.len() && !self.dinit {
                let len = unsafe { *(rbuf.as_ptr() as *const u32) } as usize;
                rbuf = unsafe {
                    let data_buf = Vec::with_capacity(len);
                    rbuf.set_len(len);
                    data_buf
                };
                self.dinit = true;
            } else if rpos == rbuf.len() && self.dinit {
                let ps = PaddingScheme::new_pkcs1v15_encrypt();
                let rem = buf.remaining();
                let decrypted = self.rsa_priv.decrypt(ps, &rbuf)?;
                if rem > decrypted.len() {
                    unsafe {
                        let unfilled = buf.initialize_unfilled();
                        std::ptr::copy(decrypted.as_ptr(), unfilled.as_mut_ptr(), decrypted.len());
                        return Poll::Ready(Ok(decrypted.len()));
                    }
                } else {
                    unsafe {
                        let unfilled = buf.initialize_unfilled();
                        std::ptr::copy(decrypted.as_ptr(), unfilled.as_mut_ptr(), unfilled.len());

                        self.cache.push_back(&decrypted[unfilled.len()..]);

                        return Poll::Ready(Ok(unfilled.len()));
                    }
                }
            }

            let mut read_buf = ReadBuf::new(&mut rbuf[rpos..]);
            match Pin::new(&mut self.target).poll_read(cx, &mut read_buf)? {
                Poll::Ready(0) => return Poll::Ready(Ok(0)),
                Poll::Ready(n) => {
                    self.rpos += n;
                }
                Poll::Pending => {
                    drop(std::mem::replace(&mut self.rbuf, Some(rbuf)));
                    return Poll::Pending;
                }
            }
        }
    }
}
