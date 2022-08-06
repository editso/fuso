mod auth;
pub use auth::*;
use std::net::SocketAddr;
use std::{pin::Pin, task::Poll};

use std::future::Future;

use crate::ext::AsyncWriteExt;
use crate::protocol::{make_packet, Poto, ToBytes};
use crate::{
    ready, Addr, AsyncRead, AsyncWrite, Kind, NetSocket, ReadBuf, Socket, SocksErr, Stream,
    UdpReceiverExt, UdpSocket,
};
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

impl<T> Socks for T where T: NetSocket + AsyncRead + AsyncWrite + Unpin {}

fn parse_address(cmd: u8, _: u8, atype: u8, data: &[u8]) -> crate::Result<Socket> {
    if cmd == 0x02 {
        return Err(SocksErr::BindNotSupport.into());
    }

    let addr = match atype {
        0x01 => {
            #[repr(C)]
            struct V4 {
                ip: [u8; 4],
                port: [u8; 2],
            }

            let v4 = unsafe { (data.as_ptr() as *mut V4).as_ref().unwrap_unchecked() };

            Addr::from((v4.ip, u16::from_be_bytes(v4.port)))
        }
        0x04 => {
            #[repr(C)]
            struct V6 {
                ip: [u8; 16],
                port: [u8; 2],
            }

            let v6 = unsafe { (data.as_ptr() as *mut V6).as_ref().unwrap_unchecked() };

            Addr::from((v6.ip, u16::from_be_bytes(v6.port)))
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
            0x01 => Socket::tcp(addr),
            0x03 => Socket::udp(addr),
            _ => return Err(SocksErr::BindNotSupport.into()),
        }
    })
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
                // +----+----------+----------+
                // |VER | NMETHODS | METHODS  |
                // +----+----------+----------+
                // | 1  |    1     | 1 to 255 |
                // +----+----------+----------+
                State::Handshake if read_buf.is_empty() => {
                    read_buf.resize(2, 0);
                }

                State::Handshake if *read_offset == 2 => {
                    let head = Head::try_from(&read_buf[..*read_offset])?;

                    log::trace!("ver={}, nmethod={}", head.ver, head.nmethod);

                    *read_offset = 0;
                    read_buf.clear();
                    read_buf.resize(head.nmethod as usize, 0);
                    drop(std::mem::replace(
                        state,
                        State::Authentication(head.nmethod as usize),
                    ));
                }
                // +----+--------+
                // |VER | METHOD |
                // +----+--------+
                // | 1  |   1    |
                // +----+--------+
                //   o  X'00' NO AUTHENTICATION REQUIRED
                //   o  X'01' GSSAPI
                //   o  X'02' USERNAME/PASSWORD
                //   o  X'03' to X'7F' IANA ASSIGNED
                //   o  X'80' to X'FE' RESERVED FOR PRIVATE METHODS
                //   o  X'FF' NO ACCEPTABLE METHODS
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

                    log::trace!("ver=0x05, cmd={}, rsv={}, atype={}", cmd, rsv, atype);

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
                    let socket = parse_address(*cmd, *rsv, *atype, &read_buf)?;

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
                State::Success(socket) if write_buf.is_empty() => {
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
                    let n = ready!(Pin::new(&mut **stream).poll_read(cx, &mut buf))?;
                    if n == 0 {
                        return Poll::Ready(Err(reset_connect!().into()));
                    }

                    *read_offset += n;

                    if *read_offset == buf.len() {
                        break;
                    }
                }
            }
        }
    }
}

pub async fn finish_udp_forward<S>(stream: &mut S) -> crate::Result<()>
where
    S: Stream + Send + Unpin,
{
    stream
        .write_all(&[0x05, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
        .await
}

pub async fn send_udp_forward_message<S>(stream: &mut S, addr: SocketAddr) -> crate::Result<()>
where
    S: Stream + Send + Unpin,
{
    let mut buf = Vec::new();

    buf.extend(&[0x05, 0x00, 0x00]);

    match addr {
        SocketAddr::V4(v1) => {
            buf.extend(&[0x01]);
            buf.extend(&v1.ip().octets());
            buf.extend(&v1.port().to_be_bytes());
        }
        SocketAddr::V6(v2) => {
            buf.extend(&[0x04]);
            buf.extend(&v2.ip().octets());
            buf.extend(&v2.port().to_be_bytes());
        }
    }

    stream.write_all(&buf).await
}

//  +----+------+------+----------+----------+----------+
//  |RSV | FRAG | ATYP | DST.ADDR | DST.PORT |   DATA   |
//  +----+------+------+----------+----------+----------+
//  | 2  |  1   |  1   | Variable |    2     | Variable |
//  +----+------+------+----------+----------+----------+
pub async fn parse_and_forward_data<S>(s1: &mut S, data: &[u8]) -> crate::Result<Addr>
where
    S: AsyncWrite + Unpin,
{
    if data.len() < 6 {
        return Err(Kind::BadForward.into());
    }

    let rsv = u16::from_be_bytes([data[0], data[1]]);
    let frag = data[2];
    let atype = data[3];

    log::trace!(
        "rsv={}, frag={}, atype={} data={}bytes",
        rsv,
        frag,
        atype,
        data.len()
    );

    if frag != 0 {
        log::warn!("frag is not supported !");
        return Err(SocksErr::Socks5Frg.into());
    }

    let (size, data) = match atype {
        0x03 => (data[4] as usize, &data[5..]),
        0x01 => (6, &data[4..]),
        0x04 => (18, &data[4..]),
        _ => return Err(SocksErr::InvalidAddress.into()),
    };

    let addr = parse_address(0x03, 0, atype, data)?.into_addr();

    let message = Poto::Forward(addr.clone()).bytes();

    s1.write_all(&message).await?;

    let data = make_packet(data[size..].to_vec()).encode();

    s1.write_all(&data).await?;

    log::trace!("send forward data success");

    Ok(addr)
}

pub async fn send_packed_udp_forward_message<U>(
    udp: &mut U,
    to: &SocketAddr,
    origin: Addr,
    data: &[u8],
) -> crate::Result<()>
where
    U: UdpSocket + Unpin,
{
    let mut buf = Vec::new();
    buf.extend(&[0x00, 0x00, 0x00]);

    match origin.into_inner() {
        crate::InnerAddr::Socket(origin) => match origin {
            SocketAddr::V4(v1) => {
                buf.push(0x01);
                buf.extend(&v1.ip().octets());
                buf.extend(&v1.port().to_be_bytes());
            }
            SocketAddr::V6(v2) => {
                buf.push(0x04);
                buf.extend(&v2.ip().octets());
                buf.extend(&v2.port().to_be_bytes());
            }
        },
        crate::InnerAddr::Domain(domain, port) => {
            let domain = domain.as_bytes();
            buf.push(0x03);
            buf.push(domain.len() as u8);
            buf.extend(domain);
            buf.extend(&port.to_be_bytes());
        }
    }

    buf.extend(data);

    match udp.send_to(to, &buf).await {
        Ok(_) => Ok(()),
        Err(e) => {
            log::warn!("udp forward fail {} {}", to, e);
            Err(e)
        }
    }
}
