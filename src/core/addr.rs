use std::{
    fmt::{Debug, Display},
    net::SocketAddr,
    str::FromStr,
};

use serde::{Deserialize, Serialize};

use crate::{Error, InvalidAddr};

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum Addr {
    Socket(SocketAddr),
    Domain(String, u16),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum Socket {
    Default,
    Udp(Addr),
    Tcp(Option<Addr>),
    Kcp(Option<Addr>),
    Quic(Option<Addr>),
}

impl From<SocketAddr> for Addr {
    fn from(addr: SocketAddr) -> Self {
        Self::Socket(addr)
    }
}

impl From<([u8; 4], u16)> for Addr {
    fn from(addr: ([u8; 4], u16)) -> Self {
        Self::Socket(SocketAddr::from(addr))
    }
}

impl From<(String, u16)> for Addr {
    fn from(addr: (String, u16)) -> Self {
        Self::Domain(addr.0, addr.1)
    }
}

impl<T> From<(&T, u16)> for Addr
where
    T: ToString,
{
    fn from(addr: (&T, u16)) -> Self {
        Self::Domain(addr.0.to_string(), addr.1)
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
        let fmt = match self {
            Addr::Socket(addr) => format!("{}", addr),
            Addr::Domain(domain, port) => format!("{}:{}", domain, port),
        };
        write!(f, "{}", fmt)
    }
}

impl Display for Socket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let fmt = match self {
            Socket::Default => format!("<socket@default>"),
            Socket::Udp(udp) => format!("<socket@udp({})>", udp),
            Socket::Tcp(tcp) => format!("<socket@tcp({:?})>", tcp),
            Socket::Kcp(kcp) => format!("<socket@kcp({:?})>", kcp),
            Socket::Quic(quic) => format!("<socket@quic({:?})>", quic),
        };

        write!(f, "{}", fmt)
    }
}

#[cfg(test)]
mod tests {
    use super::Addr;

    #[test]
    pub fn test_addr() {
        let addr = "baidu.com:80".parse::<Addr>();
        assert!(addr.is_ok());
        let addr = addr.unwrap();
        assert_eq!(addr, Addr::Domain(format!("baidu.com"), 80))
    }
}
