use std::{pin::Pin, task::Poll};

use crate::{ready, ReadBuf, SocksErr, Stream};

use super::Socks5Auth;

#[derive(Clone)]
pub enum S5Authenticate {
    Skip {
        reply: [u8; 2],
        offset: usize,
    },
    Standard {
        rpos: usize,
        rbuf: Option<Vec<u8>>,
        wpos: usize,
        wbuf: Option<[u8; 2]>,
        init: bool,
        good: bool,
        cmp_user: Option<Vec<u8>>,
        cmp_pass: Option<Vec<u8>>,
        username: Vec<u8>,
        password: Vec<u8>,
    },
}

impl S5Authenticate {
    pub fn skip() -> Self {
        Self::Skip {
            reply: [0x05, 0x00],
            offset: 0,
        }
    }

    pub fn standard<U: AsRef<[u8]>, P: AsRef<[u8]>>(username: U, password: P) -> Self {
        Self::Standard {
            rpos: 0,
            wpos: 0,
            rbuf: None,
            wbuf: Some([0x05, 0x02]),
            good: false,
            init: false,
            cmp_user: None,
            cmp_pass: None,
            username: username.as_ref().to_vec(),
            password: password.as_ref().to_vec(),
        }
    }
}

impl Default for S5Authenticate {
    fn default() -> Self {
        Self::skip()
    }
}

// The server selects from one of the methods given in METHODS, and
// sends a METHOD selection message:

//                       +----+--------+
//                       |VER | METHOD |
//                       +----+--------+
//                       | 1  |   1    |
//                       +----+--------+

// If the selected METHOD is X'FF', none of the methods listed by the
// client are acceptable, and the client MUST close the connection.

// The values currently defined for METHOD are:

//        o  X'00' NO AUTHENTICATION REQUIRED
//        o  X'01' GSSAPI
//        o  X'02' USERNAME/PASSWORD
//        o  X'03' to X'7F' IANA ASSIGNED
//        o  X'80' to X'FE' RESERVED FOR PRIVATE METHODS
//        o  X'FF' NO ACCEPTABLE METHODS
impl<S> Socks5Auth<S> for S5Authenticate
where
    S: Stream + Unpin,
{
    fn poll_auth(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context,
        mut stream: std::pin::Pin<&mut S>,
        _: &[u8],
    ) -> std::task::Poll<crate::Result<()>> {
        match &mut *self {
            S5Authenticate::Skip { reply, offset } => loop {
                let n = ready!(Pin::new(&mut *stream).poll_write(cx, &reply[*offset..]))?;

                if n == 0 {
                    return Poll::Ready(Err(std::io::Error::from(
                        std::io::ErrorKind::UnexpectedEof,
                    )
                    .into()));
                }

                *offset += n;
                if *offset == reply.len() {
                    break Poll::Ready(Ok(()));
                }
            },
            S5Authenticate::Standard {
                rpos,
                rbuf,
                wpos,
                wbuf,
                init,
                good,
                cmp_user,
                cmp_pass,
                username,
                password,
            } => {
                let mut do_next = true;
                while do_next {
                    do_next = false;
                    while let Some(buf) = wbuf.as_ref() {
                        let n = ready!(Pin::new(&mut *stream).poll_write(cx, &buf[*wpos..]))?;

                        if n == 0 {
                            return Poll::Ready(Err(std::io::Error::from(
                                std::io::ErrorKind::UnexpectedEof,
                            )
                            .into()));
                        }

                        *wpos += n;

                        if *wpos == buf.len() && cmp_pass.is_none() && cmp_user.is_none() {
                            let mut buf = Vec::new();
                            buf.resize(2, 0);
                            drop(std::mem::replace(rbuf, Some(buf)));
                            drop(std::mem::replace(wbuf, None));
                        } else if *wpos == buf.len()
                            && cmp_pass.is_some()
                            && cmp_user.is_some()
                            && !*good
                        {
                            log::debug!("socks5 authentication fail");
                            return Poll::Ready(Err(SocksErr::Authenticate.into()));
                        } else if *wpos == buf.len()
                            && cmp_pass.is_some()
                            && cmp_user.is_some()
                            && *good
                        {
                            log::debug!("socks5 authentication successful");
                            return Poll::Ready(Ok(()));
                        }
                    }

                    while let Some(buf) = rbuf.as_mut() {
                        let need = buf.len();
                        let n = ready!(Pin::new(&mut *stream)
                            .poll_read(cx, &mut ReadBuf::new(&mut buf[*rpos..])))?;

                        log::trace!("read {}, total: {}", n, buf.len());

                        if n == 0 {
                            return Poll::Ready(Err(std::io::Error::from(
                                std::io::ErrorKind::UnexpectedEof,
                            )
                            .into()));
                        }

                        *rpos += n;

                        if *rpos == need && cmp_user.is_none() && !*init {
                            // username + password len
                            let ul = buf[1] as usize + 1;

                            if buf[0] != 0x01 {
                                log::warn!("ver({}) != 0x01", buf[0]);
                                return Poll::Ready(Err(SocksErr::Authenticate.into()));
                            }

                            let mut nbuf = Vec::with_capacity(ul);

                            log::trace!("username len {}", ul);

                            unsafe {
                                nbuf.set_len(ul);
                            }

                            drop(std::mem::replace(rbuf, Some(nbuf)));
                            *rpos = 0;
                            *init = true;
                        } else if *rpos == need && cmp_user.is_none() && *init {
                            let pl = buf[buf.len() - 1] as usize;
                            let mut nbuf = Vec::with_capacity(pl);

                            log::trace!("password len {}", pl);

                            unsafe {
                                nbuf.set_len(pl);
                            }

                            *rpos = 0;

                            drop(std::mem::replace(
                                cmp_user,
                                std::mem::replace(rbuf, Some(nbuf)),
                            ))
                        } else if *rpos == need && cmp_user.is_some() {
                            drop(std::mem::replace(cmp_pass, std::mem::replace(rbuf, None)));
                            break;
                        }
                    }

                    match (&cmp_pass, &cmp_user) {
                        (Some(pass), Some(user)) => {
                            if password.eq(&pass) && user[..user.len() - 1].eq(username) {
                                *wpos = 0;
                                *good = true;
                                do_next = true;
                                drop(std::mem::replace(wbuf, Some([0x01, 0x00])));
                            } else {
                                *wpos = 0;
                                do_next = true;
                                drop(std::mem::replace(wbuf, Some([0x01, 0x01])));
                            }
                        }
                        _ => {}
                    }
                }

                // should never get this far !!!
                unsafe { std::hint::unreachable_unchecked() }
            }
        }
    }
}
