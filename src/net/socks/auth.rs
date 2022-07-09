use std::{pin::Pin, task::Poll};

use crate::{ready, Stream};

use super::Socks5Auth;

pub struct NoAuthentication {
    reply: [u8; 2],
    offset: usize,
}

impl Default for NoAuthentication {
    fn default() -> Self {
        Self {
            reply: [0x05, 0x00],
            offset: 0,
        }
    }
}

impl<S> Socks5Auth<S> for NoAuthentication
where
    S: Stream + Unpin,
{
    fn poll_auth(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context,
        mut stream: std::pin::Pin<&mut S>,
        _: &[u8],
    ) -> std::task::Poll<crate::Result<()>> {
        loop {
            let offset = self.offset;
            self.offset += ready!(Pin::new(&mut *stream).poll_write(cx, &self.reply[offset..]))?;

            if self.offset == self.reply.len() {
                break Poll::Ready(Ok(()));
            }
        }
    }
}
