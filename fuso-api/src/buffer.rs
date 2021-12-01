use std::{
    collections::VecDeque,
    io::{Cursor, Write},
    sync::{Arc, Mutex},
    task::Poll,
};

use futures::{AsyncRead, AsyncWrite};

use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct Buffer<T> {
    len: usize,
    buf: Arc<Mutex<VecDeque<Vec<T>>>>,
}

impl<T> Buffer<T>
where
    T: Clone,
{
    #[inline]
    pub fn new() -> Self {
        Self {
            len: 0,
            buf: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.buf.lock().unwrap().is_empty()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn clear(&mut self) {
        self.buf.lock().unwrap().clear();
        self.len = 0;
    }

    pub fn push_back(&mut self, data: &[T]) {
        self.buf.lock().unwrap().push_back(data.to_vec());
        self.len += data.len();
    }

    pub fn push_front(&mut self, data: &[T]) {
        self.buf.lock().unwrap().push_front(data.to_vec());
        self.len += data.len();
    }
}

impl Buffer<u8> {
    pub fn read_to_buffer(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut remaining = buf.len();
        let mut read_len = 0;
        let mut io = Cursor::new(buf);
        let mut buf = self.buf.lock().unwrap();

        loop {
            if remaining == 0 {
                self.len -= read_len;
                break Ok(read_len);
            }

            let data = buf.pop_front();

            if data.is_none() {
                self.len -= read_len;
                break Ok(read_len);
            }

            let data = data.unwrap();

            if data.len() >= remaining {
                let n = io.write(&data[..remaining])?;
                remaining -= n;
                read_len += n;

                if data.len() != n {
                    buf.push_front(data[n..].to_vec())
                }
            } else {
                let n = io.write(&data)?;

                read_len += n;
                remaining -= n;
            }
        }
    }
}

#[async_trait]
impl AsyncRead for Buffer<u8> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        Poll::Ready(self.read_to_buffer(buf))
    }
}

#[async_trait]
impl AsyncWrite for Buffer<u8> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        self.push_back(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.clear();
        Poll::Ready(Ok(()))
    }
}

#[test]
fn test_buffer() {
    use smol::io::{AsyncReadExt, AsyncWriteExt};

    smol::block_on(async move {
        let mut buf: Buffer<u8> = Buffer::new();

        buf.write(b"hello world").await.unwrap();
        buf.write(b"123456").await.unwrap();

        let mut buffer = Vec::new();
        buffer.resize(11, 0);

        let n = buf.read(&mut buffer).await.unwrap();

        assert_eq!(11, n);
        assert_eq!(6, buf.len());

        println!("{:?}", buffer);
        println!("{:?}", buf);
    });
}
