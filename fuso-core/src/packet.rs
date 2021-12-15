use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use fuso_api::Packet;

use crate::cmd::{
    CMD_ACCEPT, CMD_BIND, CMD_CONNECT, CMD_ERROR, CMD_FORWARD, CMD_PING, CMD_RESET, CMD_UDP_BIND,
    CMD_UDP_REP, CMD_UDP_REQ,
};

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
    TcpBind(Option<String>),
    UdpBind(u64),
    Reset(Addr),
    Accept(u64),
    // id
    Forward(u64, Addr),
    // conv, id
    Connect(u64, u64),
    UdpRequest(u64, Addr, Vec<u8>),
    UdpResponse(u64, Vec<u8>),
    Err(String),
    Nothing,
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
                        let domain = String::from_utf8_lossy(&buf[..size]).into();
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
            CMD_BIND if packet.get_len().eq(&0) => Ok(Action::TcpBind(None)),
            CMD_BIND => Ok(Action::TcpBind({
                let data = packet.get_mut_data();

                let len = data.get_u8();

                if len > 0 && data.len() >= len as usize {
                    let name = Some(String::from_utf8_lossy(&data[..len as usize]).to_string());
                    data.advance(len as usize);
                    name
                } else {
                    None
                }
            })),
            CMD_ACCEPT if packet.get_len().ge(&8) => {
                Ok(Action::Accept(packet.get_mut_data().get_u64()))
            }
            CMD_RESET => Ok(Action::Reset(packet.get_data().as_ref().try_into()?)),
            CMD_CONNECT if packet.get_len().ge(&16) => {
                let data = packet.get_mut_data();
                Ok(Action::Connect(data.get_u64(), data.get_u64()))
            }
            CMD_FORWARD if packet.get_len().ge(&8) => {
                let data = packet.get_mut_data();
                Ok(Action::Forward(data.get_u64(), data.as_ref().try_into()?))
            }
            CMD_ERROR => Ok(Action::Err(
                String::from_utf8_lossy(packet.get_data()).into(),
            )),
            CMD_UDP_REQ if packet.get_len().ge(&14) => {
                let data = packet.get_mut_data();

                let id = data.get_u64();
                let len = data.get_u32();

                if data.len() < len as usize {
                    return Err(fuso_api::ErrorKind::BadPacket.into());
                }

                let addr: Addr = data.as_ref().try_into()?;
                data.advance(len as usize);

                let len = data.get_u32();

                if data.len() < len as usize {
                    return Err(fuso_api::ErrorKind::BadPacket.into());
                }

                Ok(Action::UdpRequest(id, addr, data[..len as usize].to_vec()))
            }
            CMD_UDP_REP if packet.get_len().ge(&8) => {
                let data = packet.get_mut_data();
                let id = data.get_u64();
                let len = data.get_u32();

                Ok(Action::UdpResponse(id, data[..len as usize].to_vec()))
            }
            CMD_UDP_BIND if packet.get_len().ge(&8) => {
                Ok(Action::UdpBind(packet.get_mut_data().get_u64()))
            }
            _ => Err(fuso_api::ErrorKind::BadPacket.into()),
        }
    }
}

impl From<Action> for fuso_api::Packet {
    fn from(packet: Action) -> Self {
        match packet {
            Action::Ping => Packet::new(CMD_PING, Bytes::new()),
            Action::TcpBind(Some(config)) => Packet::new(CMD_BIND, {
                let mut buf = BytesMut::new();
                let config = config.as_bytes();
                buf.put_u8(config.len() as u8);
                buf.put_slice(config);

                buf.into()
            }),
            Action::TcpBind(None) => Packet::new(CMD_BIND, Bytes::new()),
            Action::Reset(addr) => Packet::new(CMD_CONNECT, addr.to_bytes()),
            Action::Accept(conv) => Packet::new(CMD_ACCEPT, {
                let mut buf = BytesMut::new();
                buf.put_u64(conv);
                buf.into()
            }),
            Action::Connect(conv, id) => Packet::new(CMD_CONNECT, {
                let mut buf = BytesMut::new();
                buf.put_u64(conv); // conv
                buf.put_u64(id); // id
                buf.into()
            }),
            Action::Forward(id, addr) => Packet::new(CMD_FORWARD, {
                let mut buf = BytesMut::new();
                buf.put_u64(id);
                buf.put_slice(&addr.to_bytes());
                buf.into()
            }),
            Action::Err(e) => Packet::new(CMD_ERROR, e.into()),
            Action::Nothing => Packet::new(CMD_RESET, Bytes::new()),
            Action::UdpRequest(id, addr, data) => Packet::new(CMD_UDP_REQ, {
                let mut buf = BytesMut::new();
                let addr = addr.to_bytes();
                buf.put_u64(id);
                buf.put_u32(addr.len() as u32);
                buf.put_slice(&addr);
                buf.put_u32(data.len() as u32);
                buf.put_slice(&data);
                buf.into()
            }),
            Action::UdpResponse(id, data) => Packet::new(CMD_UDP_REP, {
                let mut buf = BytesMut::new();
                buf.put_u64(id);
                buf.put_u32(data.len() as u32);
                buf.put_slice(&data);
                buf.into()
            }),
            Action::UdpBind(conv) => Packet::new(CMD_UDP_BIND, {
                let mut buf = BytesMut::new();
                buf.put_u64(conv);
                buf.into()
            }),
        }
    }
}

impl From<fuso_socks::Addr> for Addr {
    fn from(addr: fuso_socks::Addr) -> Self {
        match addr {
            fuso_socks::Addr::Socket(addr) => Self::Socket(addr),
            fuso_socks::Addr::Domain(domain, port) => Self::Domain(domain, port),
        }
    }
}

impl From<Addr> for fuso_socks::Addr {
    fn from(addr: Addr) -> Self {
        match addr {
            Addr::Socket(addr) => Self::Socket(addr),
            Addr::Domain(domain, port) => Self::Domain(domain, port),
        }
    }
}
