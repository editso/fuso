mod auth;
pub use auth::*;

use std::{pin::Pin, task::Poll};

use std::future::Future;

use crate::{ready, Addr, AsyncRead, AsyncWrite, ReadBuf, Socket, SocksErr, Stream};
use std::task::Context;

macro_rules! reset_connect {
    () => {
        Into::<std::io::Error>::into(std::io::ErrorKind::ConnectionReset)
    };
}

macro_rules! expect {
    ($ver: expr) => {
        if $ver != 0x05 {
            return Poll::Ready(Err(SocksErr::Protocol.into()));
        }
    };
}

macro_rules! atype_len {
    ($atype: expr) => {
        match $atype {
            0x01 => 6,  // ipv4 + port,
            0x03 => 1,  // domain
            0x04 => 18, // ipv6,
            _ => return Poll::Ready(Err(SocksErr::InvalidAddress.into())),
        }
    };
}

enum State {
    Handshake,
    // nmethod
    Authentication(usize),
    // ver, cmd, rsv, atype, size
    Request(u8, u8, u8, u8, usize),
    Success(Option<Socket>),
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Head {
    ver: u8,
    nmethod: u8,
}

#[derive(Clone, Debug, Copy)]
pub enum Method {
    // no auth
    No,
    /// gssapi
    GSSAPI,
    /// username and password
    User,
    /// iana
    IANA(u8),
    /// private
    Private,
    ///
    NotSupport,
}

#[pin_project::pin_project]
pub struct Socks5<'a, S, A> {
    state: State,
    read_buf: Vec<u8>,
    write_buf: Vec<u8>,
    read_offset: usize,
    write_offset: usize,
    auth: &'a mut A,
    #[pin]
    stream: &'a mut S,
}

pub trait Socks: Stream {
    fn socks5_handshake<'a, A>(&'a mut self, auth: &'a mut A) -> Socks5<'a, Self, A>
    where
        Self: Sized + Unpin,
        A: Socks5Auth<Self> + Unpin,
    {
        Socks5 {
            auth,
            state: State::Handshake,
            read_buf: Default::default(),
            write_buf: Default::default(),
            read_offset: 0,
            write_offset: 0,
            stream: self,
        }
    }
}

pub trait Socks5Auth<S> {
    fn poll_auth(
        self: Pin<&mut Self>,
        cx: &mut Context,
        stream: Pin<&mut S>,
        methods: &[u8],
    ) -> Poll<crate::Result<()>>;
}

impl TryFrom<u8> for Method {
    type Error = crate::Error;

    fn try_from(method: u8) -> Result<Self, Self::Error> {
        Ok(match method {
            0x00 => Self::No,
            0x01 => Self::GSSAPI,
            0x02 => Self::User,
            0x80 => Self::Private,
            0xFF => Self::NotSupport,
            m if m >= 0x03 && m <= 0x0A => Self::IANA(m),
            _ => return Err(SocksErr::Method(method).into()),
        })
    }
}

impl<'a> TryFrom<&'a [u8]> for Head {
    type Error = crate::Error;

    fn try_from(value: &'a [u8]) -> Result<Self, Self::Error> {
        if value.len() < 2 {
            return Err({
                SocksErr::BadLength {
                    expect: 2,
                    current: value.len(),
                }
                .into()
            });
        }

        let head = unsafe { (value.as_ptr() as *mut Self).as_ref().unwrap_unchecked() };

        if head.ver != 0x05 {
            return Err(SocksErr::Head {
                ver: head.ver,
                nmethod: head.nmethod,
            }
            .into());
        }

        return Ok(head.clone());
    }
}

impl<T> Socks for T where T: AsyncRead + AsyncWrite + Unpin {}

impl<'a, S, A> Socks5<'a, S, A> {
    fn parse_address(cmd: u8, _: u8, atype: u8, data: &[u8]) -> crate::Result<Socket> {
        if cmd == 0x02 {
            return Err(SocksErr::BindNotSupport.into());
        }

        let addr = match atype {
            0x01 => {
                struct V4 {
                    ip: [u8; 4],
                    port: u16,
                }

                let v4 = unsafe { (data.as_ptr() as *mut V4).as_ref().unwrap_unchecked() };

                Addr::from((v4.ip, v4.port))
            }
            0x04 => {
                struct V6 {
                    ip: [u8; 16],
                    port: u16,
                }

                let v6 = unsafe { (data.as_ptr() as *mut V6).as_ref().unwrap_unchecked() };

                Addr::from((v6.ip, v6.port))
            }
            0x03 => {
                let len = data.len();
                let port = u16::from_be_bytes([data[len - 2], data[len - 1]]);
                let domain = String::from_utf8_lossy(&data[..len - 2]);
                Addr::from((format!("{}", domain), port))
            }
            _ => return Err(SocksErr::InvalidAddress.into()),
        };

        Ok({
            match cmd {
                0x01 => Socket::Tcp(addr),
                0x03 => Socket::Udp(addr),
                _ => return Err(SocksErr::BindNotSupport.into()),
            }
        })
    }
}

impl<'a, S, A> Future for Socks5<'a, S, A>
where
    S: Stream + Unpin,
    A: Socks5Auth<S> + Unpin,
{
    type Output = crate::Result<Socket>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = self.project();
        let mut stream = this.stream;
        let auth = this.auth;
        let read_buf = this.read_buf;
        let write_buf = this.write_buf;
        let state = this.state;
        let read_offset = this.read_offset;
        let write_offset = this.write_offset;

        loop {
            match state {
                State::Handshake if read_buf.is_empty() => {
                    read_buf.resize(2, 0);
                }
                State::Handshake if *read_offset == 2 => {
                    let head = Head::try_from(&read_buf[..*read_offset])?;
                    *read_offset = 0;
                    read_buf.clear();
                    read_buf.resize(head.nmethod as usize, 0);
                    drop(std::mem::replace(
                        state,
                        State::Authentication(head.nmethod as usize),
                    ));
                }
                State::Authentication(methods) if *methods == *read_offset => {
                    ready!(Pin::new(&mut **auth).poll_auth(
                        cx,
                        Pin::new(&mut **stream),
                        &read_buf[..*methods]
                    ))?;
                    *read_offset = 0;
                    read_buf.clear();
                    read_buf.resize(4, 0);
                    drop(std::mem::replace(state, State::Request(0x05, 0, 0, 0, 4)));
                }
                State::Request(0x05, 0, 0, 0, 4) if 4 == *read_offset => {
                    expect!(read_buf[0]);

                    let cmd = read_buf[1];
                    let rsv = read_buf[2];
                    let atype = read_buf[3];
                    let len = atype_len!(atype);

                    log::debug!("ver=0x05, cmd={}, rsv={}, atype={}", cmd, rsv, atype);

                    *read_offset = 0;
                    read_buf.clear();
                    read_buf.resize(len, 0);

                    drop(std::mem::replace(
                        state,
                        State::Request(0x05, cmd, rsv, atype, len),
                    ));
                }
                State::Request(0x05, cmd, rsv, 0x03, 1) if 1 == *read_offset => {
                    *read_offset = 0;
                    let len = (read_buf[0] + 2) as usize;
                    read_buf.clear();
                    read_buf.resize(len, 0);
                    let new_state = State::Request(0x05, *cmd, *rsv, 0x03, len);
                    drop(std::mem::replace(state, new_state))
                }
                State::Request(0x05, cmd, rsv, atype, n) if n == read_offset => {
                    let socket = Self::parse_address(*cmd, *rsv, *atype, &read_buf)?;

                    log::debug!("target = {}", socket);

                    *read_offset = 0;
                    read_buf.clear();

                    if socket.is_udp() {
                        write_buf.clear();
                    } else {
                        *write_offset = 0;
                        write_buf.clear();
                        write_buf
                            .extend([0x05, 0x00, *rsv, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
                    }

                    let new_state = State::Success(Some(socket));
                    drop(std::mem::replace(state, new_state))
                }
                State::Success(socket) if *write_offset == write_buf.len() => {
                    break Poll::Ready(Ok(unsafe { socket.take().unwrap_unchecked() }));
                }
                _ => {}
            }

            if !write_buf.is_empty() {
                loop {
                    let n = ready!(
                        Pin::new(&mut **stream).poll_write(cx, &write_buf[*write_offset..])
                    )?;

                    if n == 0 {
                        return Poll::Ready(Err(reset_connect!().into()));
                    }

                    if n == write_buf.len() {
                        *write_offset = 0;
                        write_buf.clear();
                        break;
                    }
                }
            }

            if !read_buf.is_empty() {
                loop {
                    let mut buf = ReadBuf::new(&mut read_buf[*read_offset..]);
                    *read_offset += ready!(Pin::new(&mut **stream).poll_read(cx, &mut buf))?;
                    if *read_offset == 0 {
                        return Poll::Ready(Err(reset_connect!().into()));
                    } else if *read_offset == buf.len() {
                        break;
                    }
                }
            }
        }
    }
}
