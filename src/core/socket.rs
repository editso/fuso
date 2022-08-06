use std::{
    fmt::{Debug, Display},
    net::{IpAddr, SocketAddr},
    ops::{Add, Deref, DerefMut},
    str::FromStr,
};

use rand::seq::{IteratorRandom};
use serde::{Deserialize, Serialize};

use crate::{Error, InvalidAddr, Kind};

macro_rules! impl_socket {
    ($name: ident, $is: ident, $typ: ident) => {
        impl Socket {
            pub fn $name<A: Into<Addr>>(addr: A) -> Self {
                Self {
                    target: addr.into(),
                    kind: SocketKind::$typ,
                    is_mixed: false,
                }
            }

            pub fn $is(&self) -> bool {
                self.kind == SocketKind::$typ
            }
        }
    };
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum Address {
    One(Socket),
    Many(Vec<Socket>),
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum InnerAddr {
    Socket(SocketAddr),
    Domain(String, u16),
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Addr(InnerAddr);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum SocketKind {
    Kcp,
    Udp,
    Tcp,
    Quic,
    /// udp forward
    Ufd,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Socket {
    kind: SocketKind,
    target: Addr,
    is_mixed: bool,
}

impl_socket!(udp, is_udp, Udp);
impl_socket!(kcp, is_kcp, Kcp);
impl_socket!(tcp, is_tcp, Tcp);
impl_socket!(quic, is_quic, Quic);
impl_socket!(ufd, is_ufd, Ufd);

impl From<SocketAddr> for Addr {
    fn from(addr: SocketAddr) -> Self {
        Self(InnerAddr::Socket(addr))
    }
}

impl From<([u8; 4], u16)> for Addr {
    fn from(addr: ([u8; 4], u16)) -> Self {
        Self(InnerAddr::Socket(SocketAddr::from(addr)))
    }
}

impl From<([u8; 16], u16)> for Addr {
    fn from(addr: ([u8; 16], u16)) -> Self {
        Self(InnerAddr::Socket(SocketAddr::from(addr)))
    }
}

impl From<(String, u16)> for Addr {
    fn from(addr: (String, u16)) -> Self {
        Self(InnerAddr::Domain(addr.0, addr.1))
    }
}

impl From<(IpAddr, u16)> for Addr {
    fn from(addr: (IpAddr, u16)) -> Self {
        Self(InnerAddr::Socket((addr.0, addr.1).into()))
    }
}

impl From<u16> for Addr {
    fn from(port: u16) -> Self {
        ([0, 0, 0, 0], port).into()
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let fmt = match self {
            Address::One(addr) => format!("{}", addr),
            Address::Many(address) => {
                format!("{}", {
                    address
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<String>>()
                        .join(" | ")
                })
            }
        };

        write!(f, "{}", fmt)
    }
}

impl FromStr for Addr {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.parse::<SocketAddr>() {
            Ok(socket) => Ok(socket.into()),
            Err(e) => {
                let index = s
                    .find(":")
                    .ok_or_else(|| Error::from(InvalidAddr::Socket(e)))?;

                let (host, port) = s.split_at(index + 1);
                let port = port
                    .parse::<u16>()
                    .map_err(|_| Error::from(InvalidAddr::Domain(format!("{}", s))))?;

                log::debug!("{}:{}", host, port);

                Ok((host.replace(":", ""), port).into())
            }
        }
    }
}

impl Debug for Addr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl Display for Addr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let fmt = match &self.0 {
            InnerAddr::Socket(addr) => {
                if addr.ip().is_unspecified() {
                    format!("[::{}]", addr.port())
                } else {
                    format!("{}", addr)
                }
            }
            InnerAddr::Domain(domain, port) => format!("{}:{}", domain, port),
        };
        write!(f, "{}", fmt)
    }
}

impl SocketKind {
    pub fn is_kcp(&self) -> bool {
        self == &Self::Kcp
    }

    pub fn is_udp(&self) -> bool {
        self == &Self::Udp
    }

    pub fn is_tcp(&self) -> bool {
        self == &Self::Tcp
    }

    pub fn is_quic(&self) -> bool {
        self == &Self::Quic
    }
}

#[cfg(debug_assertions)]
impl Display for SocketKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let fmt = match self {
            SocketKind::Kcp => "KCP",
            SocketKind::Udp => "UDP",
            SocketKind::Tcp => "TCP",
            SocketKind::Quic => "QUIC",
            SocketKind::Ufd => "UFD",
        };

        write!(f, "{}", fmt)
    }
}

#[cfg(not(debug_assertions))]
impl Display for SocketKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let fmt = match self {
            SocketKind::Kcp => "K",
            SocketKind::Udp => "U",
            SocketKind::Tcp => "T",
            SocketKind::Quic => "Q",
            SocketKind::Ufd => "F",
        };

        write!(f, "{}", fmt)
    }
}

#[cfg(debug_assertions)]
impl Display for Socket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let fmt_addr = {
            if self.is_default() {
                format!("--")
            } else {
                format!("{}", self.addr())
            }
        };

        if self.is_mixed() && !self.is_ufd() {
            write!(f, "{} MIX", fmt_addr)
        } else {
            write!(f, "{} {}", fmt_addr, self.kind)
        }
    }
}

#[cfg(not(debug_assertions))]
impl Display for Socket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let fmt_addr = {
            if self.is_default() {
                format!("--")
            } else {
                format!("{}", self.addr())
            }
        };

        if self.is_mixed() && !self.is_ufd() {
            write!(f, "{} M", fmt_addr)
        } else {
            write!(f, " {} {}", fmt_addr, self.kind)
        }
    }
}

impl Addr {
    pub fn as_string(&self) -> String {
        match &self.0 {
            InnerAddr::Socket(addr) => format!("{}", addr),
            InnerAddr::Domain(host, port) => format!("{}:{}", host, port),
        }
    }

    pub fn is_ip(&self) -> bool {
        match &self.0 {
            InnerAddr::Socket(_) => true,
            InnerAddr::Domain(_, _) => false,
        }
    }

    pub fn is_domain(&self) -> bool {
        match self.0 {
            InnerAddr::Socket(_) => false,
            InnerAddr::Domain(_, _) => true,
        }
    }

    pub fn ip(&self) -> Option<IpAddr> {
        match &self.0 {
            InnerAddr::Socket(addr) => Some(addr.ip()),
            InnerAddr::Domain(_, _) => None,
        }
    }

    pub fn domain(&self) -> Option<&str> {
        match &self.0 {
            InnerAddr::Socket(_) => None,
            InnerAddr::Domain(domain, _) => Some(domain),
        }
    }

    pub fn is_ip_unspecified(&self) -> bool {
        match self.0 {
            InnerAddr::Domain(_, _) => false,
            InnerAddr::Socket(socket) => socket.ip().is_unspecified(),
        }
    }

    pub fn port(&self) -> u16 {
        match &self.0 {
            InnerAddr::Socket(addr) => addr.port(),
            InnerAddr::Domain(_, port) => *port,
        }
    }

    pub fn set_ip<IP: Into<IpAddr>>(&mut self, ip: IP) {
        self.0 = InnerAddr::Socket(SocketAddr::new(ip.into(), self.port()));
    }

    pub fn from_set_host(&mut self, socket: &Addr) {
        if socket.is_domain() {
            self.set_domain(unsafe { socket.domain().unwrap_unchecked() });
        } else {
            self.set_ip(unsafe { socket.ip().unwrap_unchecked() })
        }
    }

    pub fn set_domain(&mut self, domain: &str) {
        self.0 = InnerAddr::Domain(format!("{}", domain), self.port());
    }

    pub fn set_port(&mut self, new_port: u16) {
        match &mut self.0 {
            InnerAddr::Socket(socket) => {
                socket.set_port(new_port);
            }
            InnerAddr::Domain(_, old_port) => {
                *old_port = new_port;
            }
        }
    }

    pub fn is_default(&self) -> bool {
        match &self.0 {
            InnerAddr::Domain(_, _) => false,
            InnerAddr::Socket(addr) => addr.port() == 0 && addr.ip().is_unspecified(),
        }
    }

    pub fn inner(&self) -> &InnerAddr {
        &self.0
    }

    pub fn into_inner(self) -> InnerAddr {
        self.0
    }
}

impl Socket {
    pub fn is_mixed(&self) -> bool {
        self.is_mixed
    }

    pub fn into_addr(self) -> Addr {
        self.target
    }

    pub fn addr(&self) -> &Addr {
        &self
    }

    pub fn kind(&self) -> SocketKind {
        self.kind
    }

    pub fn with_kind(mut self, kind: SocketKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn if_stream_mixed(mut self, mixed: bool) -> Self {
        self.is_mixed = mixed;
        self
    }

    pub fn default_or<S: Into<Self>>(self, socket: S) -> Self {
        if self.is_default() {
            socket
                .into()
                .if_stream_mixed(self.is_mixed)
                .with_kind(self.kind)
        } else {
            self
        }
    }
}

impl Address {
    pub fn if_stream_mixed(self, mixed: bool) -> Self {
        match self {
            Address::One(socket) => Address::One(socket.if_stream_mixed(mixed)),
            Address::Many(addrs) => Address::Many(
                addrs
                    .into_iter()
                    .map(|addr| addr.if_stream_mixed(mixed))
                    .collect(),
            ),
        }
    }

    pub fn with_kind(self, kind: SocketKind) -> Self {
        match self {
            Address::One(socket) => Address::One(socket.with_kind(kind)),
            Address::Many(addrs) => {
                Address::Many(addrs.into_iter().map(|addr| addr.with_kind(kind)).collect())
            }
        }
    }

    pub fn set_ip<IP: Into<IpAddr>>(&mut self, ip: IP) {
        match self {
            Address::One(socket) => socket.set_ip(ip),
            Address::Many(addrs) => {
                let ip: IpAddr = ip.into();
                for addr in addrs {
                    addr.set_ip(ip.clone())
                }
            }
        }
    }

    pub fn from_set_host(&mut self, addr: &Addr) {
        match self {
            Address::One(socket) => socket.from_set_host(addr),
            Address::Many(addrs) => {
                for socket in addrs {
                    socket.from_set_host(addr)
                }
            }
        }
    }

    pub fn is_ip_unspecified(&self) -> bool {
        match self {
            Address::One(socket) => socket.is_ip_unspecified(),
            Address::Many(sockets) => {
                if let Some(socket) = sockets.first() {
                    socket.is_ip_unspecified()
                } else {
                    false
                }
            }
        }
    }
}

impl Deref for Socket {
    type Target = Addr;

    fn deref(&self) -> &Self::Target {
        &self.target
    }
}

impl DerefMut for Socket {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.target
    }
}

impl Default for Socket {
    fn default() -> Self {
        Self {
            target: 0.into(),
            kind: SocketKind::Tcp,
            is_mixed: false,
        }
    }
}

impl Address {
    pub fn select(&self, socket: &Socket) -> crate::Result<Socket> {
        match self {
            Address::One(socket) => Ok(socket.clone()),
            Address::Many(addrs) => {
                let addrs = addrs.into_iter().filter(|addr| match socket.kind() {
                    SocketKind::Kcp => addr.is_kcp(),
                    SocketKind::Udp => addr.is_udp(),
                    SocketKind::Tcp => addr.is_tcp(),
                    SocketKind::Quic => addr.is_quic(),
                    SocketKind::Ufd => {
                        addr.is_kcp()
                            || addr.is_ufd()
                            || addr.is_udp()
                            || addr.is_tcp()
                            || addr.is_quic()
                    }
                });

                addrs
                    .clone()
                    .filter(|addr| addr.is_kcp())
                    .choose(&mut rand::thread_rng())
                    .or_else(|| addrs.choose(&mut rand::thread_rng()))
                    .map(Clone::clone)
                    .ok_or_else(|| Error::from(Kind::Improper(socket.clone())))
            }
        }
    }
}

impl Add for Address {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        let mut address = Vec::new();

        match self {
            Address::One(socket) => address.push(socket),
            Address::Many(sockets) => address = sockets,
        }

        match rhs {
            Address::One(socket) => address.push(socket),
            Address::Many(sockets) => address.extend(sockets),
        }

        Self::Many(address)
    }
}
