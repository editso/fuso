use std::task::Poll;

use tokio::io::ReadBuf;

use crate::core::io::{AsyncRead, AsyncWrite};

impl AsyncRead for tokio::io::Stdin {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        let mut buf = ReadBuf::new(buf);
        match tokio::io::AsyncRead::poll_read(self, cx, &mut buf)? {
            Poll::Pending => Poll::Pending,
            Poll::Ready(()) => Poll::Ready(Ok(buf.filled().len())),
        }
    }
}

impl AsyncWrite for tokio::io::Stdout {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<crate::error::Result<usize>> {
        tokio::io::AsyncWrite::poll_write(self, cx, buf).map_err(Into::into)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<crate::error::Result<()>> {
        tokio::io::AsyncWrite::poll_flush(self, cx).map_err(Into::into)
    }
}

#[cfg(windows)]
mod windows {
    use std::task::Poll;

    use tokio::io::ReadBuf;

    use crate::{
        core::io::{AsyncRead, AsyncWrite},
        error,
        pty::{self, OpenPty, OpenedPty, PtyBuilder},
    };

    struct WithTokio;

    impl PtyBuilder {
        pub fn build(self) -> error::Result<OpenedPty> {
            self.build_with::<WithTokio>()
        }
    }

    impl OpenPty for WithTokio {
        fn connect_input(input: String) -> error::Result<crate::pty::AbstractPtyInput> {
            tokio::net::windows::named_pipe::ClientOptions::new()
                .open(input)
                .map(pty::AbstractPtyInput::new)
                .map_err(Into::into)
        }

        fn connect_output(output: String) -> error::Result<crate::pty::AbstractPtyOutput> {
            tokio::net::windows::named_pipe::ClientOptions::new()
                .open(output)
                .map(pty::AbstractPtyOutput::new)
                .map_err(Into::into)
        }
    }

    impl AsyncRead for tokio::net::windows::named_pipe::NamedPipeClient {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut [u8],
        ) -> std::task::Poll<crate::error::Result<usize>> {
            let mut buf = ReadBuf::new(buf);
            match tokio::io::AsyncRead::poll_read(self, cx, &mut buf)? {
                Poll::Pending => Poll::Pending,
                Poll::Ready(()) => Poll::Ready(Ok(buf.filled().len())),
            }
        }
    }

    impl AsyncWrite for tokio::net::windows::named_pipe::NamedPipeClient {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<crate::error::Result<usize>> {
            tokio::io::AsyncWrite::poll_write(self, cx, buf).map_err(Into::into)
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<crate::error::Result<()>> {
            tokio::io::AsyncWrite::poll_flush(self, cx).map_err(Into::into)
        }
    }
}

#[cfg(not(windows))]
mod unix {
    use std::{sync::Arc, task::Poll};

    use crate::{
        core::{
            io::{AsyncRead, AsyncWrite},
            split::SplitStream,
        },
        error,
        pty::{self, OpenPty, OpenedPty, PtyBuilder},
    };

    extern "C" {
        fn __errno_location() -> *mut i32;
    }

    #[derive(Clone)]
    struct Pty(Arc<tokio::io::unix::AsyncFd<i32>>);

    struct WithTokio;

    impl PtyBuilder {
        pub fn build(self) -> error::Result<OpenedPty> {
            self.build_with::<WithTokio>()
        }
    }

    impl OpenPty for WithTokio {
        fn pipe(input: i32) -> error::Result<(pty::AbstractPtyInput, pty::AbstractPtyOutput)> {
            let pty = Pty(Arc::new(tokio::io::unix::AsyncFd::new(input)?));
            let (output, input) = pty.split();
            Ok((
                pty::AbstractPtyInput::new(input),
                pty::AbstractPtyOutput::new(output),
            ))
        }
    }

    impl AsyncRead for Pty {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut [u8],
        ) -> std::task::Poll<crate::error::Result<usize>> {
            extern "C" {
                fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
            }

            match self.0.poll_read_ready(cx)? {
                std::task::Poll::Pending => Poll::Pending,
                std::task::Poll::Ready(mut ar) => unsafe {
                    let n = read(*ar.get_inner(), buf.as_ptr() as _, buf.len());
                    ar.clear_ready();
                    if n < 0 {
                        Poll::Ready(Err(
                            std::io::Error::from_raw_os_error(*__errno_location()).into()
                        ))
                    } else {
                        Poll::Ready(Ok(n as usize))
                    }
                },
            }
        }
    }

    impl AsyncWrite for Pty {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<crate::error::Result<usize>> {
            extern "C" {
                fn write(fd: i32, buf: *const u8, count: usize) -> isize;
            }

            match self.0.poll_write_ready(cx)? {
                std::task::Poll::Pending => Poll::Pending,
                std::task::Poll::Ready(mut ar) => unsafe {
                    let n = write(*ar.get_inner(), buf.as_ptr(), buf.len());
                    ar.clear_ready();
                    if n < 0 {
                        Poll::Ready(Err(
                            std::io::Error::from_raw_os_error(*__errno_location()).into()
                        ))
                    } else {
                        Poll::Ready(Ok(n as usize))
                    }
                },
            }
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<crate::error::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
}
