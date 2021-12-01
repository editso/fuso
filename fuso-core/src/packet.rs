use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use fuso_api::Packet;

use crate::cmd::{CMD_ACCEPT, CMD_BIND, CMD_CONNECT, CMD_ERROR, CMD_FORWARD, CMD_PING, CMD_RESET};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Addr {
    Socket(SocketAddr),
    Domain(String, u16),
}

/// 建立连接过程
/// client ---> cmd(bind) ---> server
/// auth....
/// server ---> cmd(accept) ----> client  

/// 建立映射过程
/// server ---> forward ----> client
/// client ---> connect ----> server

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Ping,
    Bind(Option<SocketAddr>),
    Reset(Addr),
    Accept(u64),
    Forward(Addr),
    Connect(u64),
    Err(String),
}

impl Addr {
    pub fn to_bytes(&self) -> Bytes {
        let mut buf = BytesMut::new();
        match self {
            Addr::Socket(SocketAddr::V4(v4)) => {
                buf.put_u8(0x01); // ipv4
                buf.put_slice(&v4.ip().octets());
                buf.put_u16(v4.port());
            }
            Addr::Socket(SocketAddr::V6(v6)) => {
                buf.put_u8(0x02); // ipv6
                buf.put_slice(&v6.ip().octets());
                buf.put_u16(v6.port());
            }
            Addr::Domain(domain, port) => {
                buf.put_u8(0x03); // domain
                buf.put_u8(domain.len() as u8);
                buf.put_slice(domain.as_bytes());
                buf.put_u16(*port);
            }
        }
        buf.into()
    }
}

impl From<SocketAddr> for Addr {
    #[inline]
    fn from(addr: SocketAddr) -> Self {
        Self::Socket(addr)
    }
}

impl TryFrom<&[u8]> for Addr {
    type Error = fuso_api::Error;

    fn try_from(raw: &[u8]) -> Result<Self, fuso_api::Error> {
        let mut buf: BytesMut = raw.clone().into();
        match raw {
            // ipv4
            &[0x01, ..] if buf.len() >= 6 => Ok(Self::Socket(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::from({
                    buf.advance(1);
                    buf.get_u32()
                })),
                buf.get_u16(),
            ))),
            // ipv6
            &[0x02, ..] if buf.len() >= 18 => Ok(Self::Socket(SocketAddr::new(
                IpAddr::V6(Ipv6Addr::from({
                    buf.advance(1);
                    buf.get_u128()
                })),
                buf.get_u16(),
            ))),
            &[0x3, ..] if buf.len() >= 2 => Ok(Addr::Domain(
                {
                    buf.advance(1);
                    let size = buf.get_u8() as usize;

                    if buf.len() <= size {
                        return Err(fuso_api::ErrorKind::BadPacket.into());
                    } else {
                        let domain = String::from_utf8_lossy(&buf[1..size]).into();
                        buf.advance(size);
                        domain
                    }
                },
                buf.get_u16(),
            )),
            &[..] => Err(fuso_api::ErrorKind::BadPacket.into()),
        }
    }
}

impl TryFrom<Packet> for Action {
    type Error = fuso_api::Error;

    #[inline]
    fn try_from(mut packet: fuso_api::Packet) -> Result<Self, Self::Error> {
        match packet.get_cmd() {
            CMD_PING => Ok(Action::Ping),
            CMD_BIND if packet.get_len().eq(&0) => Ok(Action::Bind(None)),
            CMD_BIND => {
                let addr: Addr = packet.get_data().as_ref().try_into()?;
                Ok(Action::Bind(Some({
                    match addr {
                        Addr::Socket(addr) => addr,
                        Addr::Domain(_, _) => {
                            return Err(fuso_api::Error::with_io(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "Does not support domain bind",
                            )))
                        }
                    }
                })))
            }
            CMD_ACCEPT if packet.get_len().ge(&8) => {
                Ok(Action::Accept(packet.get_mut_data().get_u64()))
            }
            CMD_RESET => Ok(Action::Reset(packet.get_data().as_ref().try_into()?)),
            CMD_CONNECT if packet.get_len().ge(&8) => {
                Ok(Action::Connect(packet.get_mut_data().get_u64()))
            }
            CMD_FORWARD if packet.get_len().ge(&0) => {
                Ok(Action::Forward(packet.get_data().as_ref().try_into()?))
            }
            CMD_ERROR => Ok(Action::Err(
                String::from_utf8_lossy(packet.get_data()).into(),
            )),
            _ => Err(fuso_api::ErrorKind::BadPacket.into()),
        }
    }
}

impl From<Action> for fuso_api::Packet {
    fn from(packet: Action) -> Self {
        match packet {
            Action::Ping => Packet::new(CMD_PING, Bytes::new()),
            Action::Bind(Some(addr)) => Packet::new(CMD_BIND, Addr::Socket(addr).to_bytes()),
            Action::Bind(None) => Packet::new(CMD_BIND, Bytes::new()),
            Action::Reset(addr) => Packet::new(CMD_CONNECT, addr.to_bytes()),
            Action::Accept(conv) => Packet::new(CMD_ACCEPT, {
                let mut buf = BytesMut::new();
                buf.put_u64(conv);
                buf.into()
            }),
            Action::Connect(conv) => Packet::new(CMD_CONNECT, {
                let mut buf = BytesMut::new();
                buf.put_u64(conv);
                buf.into()
            }),
            Action::Forward(addr) => Packet::new(CMD_FORWARD, addr.to_bytes()),
            Action::Err(e) => Packet::new(CMD_ERROR, e.into()),
        }
    }
}
