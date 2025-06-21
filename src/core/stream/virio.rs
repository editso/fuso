use std::{
    collections::VecDeque,
    pin::Pin,
    sync::Arc,
    task::{Poll, Waker},
};

use parking_lot::Mutex;

use crate::{
    core::io::{AsyncRead, AsyncWrite},
    error,
};

type VirWaker = Arc<Mutex<Option<Waker>>>;

#[derive(Clone, Debug)]
struct Container {
    total: usize,
    maxbuf: usize,
    buffer: VecDeque<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct VirBuf {
    container: Arc<Mutex<Container>>,
    read_waker: VirWaker,
    write_waker: VirWaker,
}

#[derive(Clone, Debug)]
pub struct Vitio {
    left: VirBuf,
    right: VirBuf,
}

impl AsyncRead for Vitio {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        Pin::new(&mut self.left).poll_read(cx, buf)
    }
}

impl AsyncWrite for Vitio {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        Pin::new(&mut self.right).poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::error::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl VirBuf {
    fn new(read_waker: VirWaker, write_waker: VirWaker) -> Self {
        Self {
            read_waker,
            write_waker,
            container: Default::default(),
        }
    }
}

impl Default for Container {
    fn default() -> Self {
        Self {
            total: 0,
            maxbuf: 5,
            buffer: Default::default(),
        }
    }
}

impl AsyncRead for VirBuf {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        let n = { self.container.lock().read(buf)? };

        let r = if n == 0 {
            self.read_waker.lock().replace(cx.waker().clone());
            Poll::Pending
        } else {
            Poll::Ready(Ok(n))
        };

        self.try_wake_write();

        r
    }
}

impl AsyncWrite for VirBuf {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        if self.container.lock().write(buf) {
            self.try_wake_read();
            Poll::Ready(Ok(buf.len()))
        } else {
            self.write_waker.lock().replace(cx.waker().clone());
            Poll::Pending
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::error::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl VirBuf {
    fn try_wake_write(&mut self) {
        if let Some(waker) = self.write_waker.lock().take() {
            waker.wake();
        }
    }

    fn try_wake_read(&mut self) {
        if let Some(waker) = self.read_waker.lock().take() {
            waker.wake();
        }
    }
}

impl Container {
    fn write(&mut self, buf: &[u8]) -> bool {
        if self.buffer.len() >= self.maxbuf {
            false
        } else {
            self.buffer.push_back(buf.to_vec());
            self.total += buf.len();
            true
        }
    }

    fn read(&mut self, buf: &mut [u8]) -> error::Result<usize> {
        let mut off = 0;

        while off < buf.len() {
            match self.buffer.pop_front() {
                None => break,
                Some(data) => {
                    let rem = buf.len() - off;
                    let total = data.len();

                    let fill = if total > rem { rem } else { total };

                    buf[off..off + fill].copy_from_slice(&data[..fill]);

                    off += fill;

                    if fill < total {
                        self.buffer.push_front(data[fill..].to_vec());
                    }
                }
            }
        }

        self.total -= off;

        Ok(off)
    }
}

pub fn open() -> (Vitio, Vitio) {
    let read_waker = VirWaker::default();
    let write_waker = VirWaker::default();

    let left = VirBuf::new(read_waker, write_waker);

    let read_waker = VirWaker::default();
    let write_waker = VirWaker::default();

    let right = VirBuf::new(read_waker, write_waker);

    let v1 = Vitio {
        left: right.clone(),
        right: left.clone(),
    };

    let v2 = Vitio { left, right };

    (v1, v2)
}

#[cfg(test)]
mod tests {
    use crate::{
        core::{
            io::{AsyncReadExt, AsyncWriteExt},
            protocol::{AsyncPacketRead, AsyncPacketSend},
            rpc::{Decoder, Encoder},
            stream::fragment::Fragment,
        },
        error,
    };

    use super::open;

    #[tokio::test]
    async fn k() {
        env_logger::builder()
            .filter_level(log::LevelFilter::Debug)
            .init();

        let (mut v1, mut v2) = open();

        // tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        println!("call2 ...");

        v1.send_packet(&Fragment::Ping.encode().unwrap())
            .await
            .unwrap();

        // let n = v2.recv_packet().await.unwrap();

        // let f: error::Result<Fragment> = f.to_vec().decode();

        // f.unwrap();

        let mut buf = [0u8; 1];

        let da = v2.recv_packet().await.unwrap();

        println!("{da:?}");

        let n = v2.read(&mut buf).await.unwrap();

        println!("{:?}", &buf[..n])
    }
}
