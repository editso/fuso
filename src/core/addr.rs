use std::{
    fmt::{Debug, Display},
    hint,
    net::{IpAddr, SocketAddr},
    ops::{Add, Deref, DerefMut},
    str::FromStr,
};

use serde::{Deserialize, Serialize};

use crate::{Error, InvalidAddr};

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum InnerAddr {
    Socket(SocketAddr),
    Domain(String, u16),
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Addr(InnerAddr);

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum Socket {
    Udp(Addr),
    Tcp(Addr),
    Kcp(Addr),
    Quic(Addr),
}

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

impl From<(String, u16)> for Addr {
    fn from(addr: (String, u16)) -> Self {
        Self(InnerAddr::Domain(addr.0, addr.1))
    }
}

impl<T> From<(&T, u16)> for Addr
where
    T: ToString,
{
    fn from(addr: (&T, u16)) -> Self {
        Self(InnerAddr::Domain(addr.0.to_string(), addr.1))
    }
}

impl From<u16> for Addr {
    fn from(port: u16) -> Self {
        ([0, 0, 0, 0], port).into()
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
            InnerAddr::Socket(addr) => format!("{}", addr),
            InnerAddr::Domain(domain, port) => format!("{}:{}", domain, port),
        };
        write!(f, "{}", fmt)
    }
}

impl Display for Socket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let fmt = match self {
            Socket::Udp(udp) => format!("<socket@udp({})>", udp),
            Socket::Tcp(tcp) => format!("<socket@tcp({:?})>", tcp),
            Socket::Kcp(kcp) => format!("<socket@kcp({:?})>", kcp),
            Socket::Quic(quic) => format!("<socket@quic({:?})>", quic),
        };

        write!(f, "{}", fmt)
    }
}

impl Addr {
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
}

impl Socket {
    pub fn default_or<A: Into<Self>>(self, addr: A) -> Self {
        if self.is_default() {
            addr.into()
        } else {
            self
        }
    }
}

impl Deref for Socket {
    type Target = Addr;

    fn deref(&self) -> &Self::Target {
        match self {
            Socket::Udp(addr) => addr,
            Socket::Tcp(addr) => addr,
            Socket::Kcp(addr) => addr,
            Socket::Quic(addr) => addr,
        }
    }
}

impl DerefMut for Socket {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Socket::Udp(addr) => addr,
            Socket::Tcp(addr) => addr,
            Socket::Kcp(addr) => addr,
            Socket::Quic(addr) => addr,
        }
    }
}

impl Default for Socket {
    fn default() -> Self {
        Self::Tcp(([0, 0, 0, 0], 0).into())
    }
}
