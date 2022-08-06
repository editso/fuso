use std::cell::RefCell;
use std::rc::Rc;
use std::{future::Future, pin::Pin, task::Poll, time::Duration};

use crate::{AsyncRead, AsyncWrite, BoxedFuture};

pub struct Timer<T> {
    target: Rc<RefCell<T>>,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    read_fut: Option<BoxedFuture<'static, crate::Result<usize>>>,
    write_fut: Option<BoxedFuture<'static, crate::Result<usize>>>,
}

unsafe impl<T> Send for Timer<T> {}
unsafe impl<T> Sync for Timer<T> {}

impl<T> Timer<T>
where
    T: 'static,
{
    pub fn into_inner(self) -> T {
        unsafe { self.target.as_ptr().read() }
    }

    pub fn new(target: T, read_timeout: Option<Duration>, write_timeout: Option<Duration>) -> Self {
        Self {
            target: Rc::new(RefCell::new(target)),
            read_timeout,
            write_timeout,
            read_fut: None,
            write_fut: None,
        }
    }

    pub fn with_read_write(target: T, timeout: Duration) -> Self {
        log::debug!(
            "[timer] overtime time read={}ms, write={}ms",
            timeout.as_millis(),
            timeout.as_millis()
        );

        Self {
            target: Rc::new(RefCell::new(target)),
            read_fut: None,
            write_fut: None,
            read_timeout: Some(timeout),
            write_timeout: Some(timeout),
        }
    }

    pub fn with_read(target: T, timeout: Duration) -> Self {
        log::debug!("[timer] overtime time read={}ms", timeout.as_millis());

        Self {
            target: Rc::new(RefCell::new(target)),
            read_fut: None,
            write_fut: None,
            read_timeout: Some(timeout),
            write_timeout: None,
        }
    }

    pub fn with_write(target: T, timeout: Duration) -> Self {
        log::debug!("[timer] overtime time write={}ms", timeout.as_millis());

        Self {
            target: Rc::new(RefCell::new(target)),
            read_fut: None,
            write_fut: None,
            read_timeout: None,
            write_timeout: Some(timeout),
        }
    }

    pub fn read_timeout(mut self, timeout: Duration) -> Self {
        self.read_timeout = Some(timeout);
        self
    }

    pub fn write_timeout(mut self, timeout: Duration) -> Self {
        self.write_timeout = Some(timeout);
        self
    }

    pub fn reset_read_timeout(mut self) -> Self {
        self.read_timeout = None;
        self
    }

    pub fn reset_write_timeout(mut self) -> Self {
        self.write_timeout = None;
        self
    }
}

#[allow(unused)]
impl<T> AsyncWrite for Timer<T>
where
    T: AsyncWrite + Unpin + 'static,
{
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::Result<usize>> {
        log::debug!("poll timer write");
        let timeout = match self.read_timeout.as_ref() {
            None => {
                let mut target = self.target.try_borrow_mut()?;
                return Pin::new(&mut *target).poll_write(cx, buf);
            }
            Some(timeout) => timeout.clone(),
        };

        let target = self.target.clone();
        let len = buf.len();
        let buf = buf.as_ptr();

        let mut fut: BoxedFuture<'static, crate::Result<usize>> = match self.write_fut.take() {
            Some(fut) => fut,
            None => Box::pin(async move {
                // let mut target = target.try_borrow_mut()?;
                // let buf = unsafe {
                //     match std::ptr::slice_from_raw_parts(buf, len).as_ref() {
                //         Some(buf) => buf,
                //         None => return Err(crate::error::Kind::Memory.into()),
                //     }
                // };

                // match time::wait_for(timeout, target.write(buf)).await {
                //     Ok(ready) => ready,
                //     Err(e) => Err({
                //         log::warn!("[timer] write timeout {}", e);
                //         std::io::ErrorKind::TimedOut.into()
                //     }),
                // }
                unimplemented!()
            }),
        };

        match Pin::new(&mut fut).poll(cx) {
            Poll::Ready(ready) => Poll::Ready(ready),
            Poll::Pending => {
                drop(std::mem::replace(&mut self.read_fut, Some(fut)));
                Poll::Pending
            }
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        let mut target = self.target.try_borrow_mut()?;
        return Pin::new(&mut *target).poll_flush(cx);
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        let mut target = self.target.try_borrow_mut()?;
        return Pin::new(&mut *target).poll_close(cx);
    }
}

#[allow(unused)]
impl<T> AsyncRead for Timer<T>
where
    T: AsyncRead + Unpin + 'static,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> std::task::Poll<crate::Result<usize>> {
        log::debug!("poll timer read");
        let timeout = match self.read_timeout.as_ref() {
            None => {
                let mut target = self.target.try_borrow_mut()?;
                return Pin::new(&mut *target).poll_read(cx, buf);
            }
            Some(timeout) => timeout.clone(),
        };

        let target = self.target.clone();
        let buf = buf.initialize_unfilled();
        let len = buf.len();
        let buf = buf.as_mut_ptr();

        let mut fut: BoxedFuture<'static, crate::Result<usize>> = match self.read_fut.take() {
            Some(fut) => fut,
            None => Box::pin(async move {
                let mut target = target.try_borrow_mut()?;
                let buf = unsafe {
                    match std::ptr::slice_from_raw_parts_mut(buf, len).as_mut() {
                        Some(buf) => buf,
                        None => return Err(crate::error::Kind::Memory.into()),
                    }
                };

                // match time::wait_for(timeout, target.read(buf)).await {
                //     Ok(ready) => ready,
                //     Err(e) => Err({
                //         log::warn!("[timer] read timeout {}", e);
                //         std::io::ErrorKind::TimedOut.into()
                //     }),
                // }
                unimplemented!()
            }),
        };

        match Pin::new(&mut fut).poll(cx) {
            Poll::Ready(ready) => Poll::Ready(ready),
            Poll::Pending => {
                drop(std::mem::replace(&mut self.read_fut, Some(fut)));
                Poll::Pending
            }
        }
    }
}
