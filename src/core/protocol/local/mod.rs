use std::{
    io::Read,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
};

use bytes::{Buf, BufMut};

use crate::{Addr, Error, Kind, Result, Socket};

// macro_rules! invalid {
//     () => {
//         Err(Kind::Deserialize("invalid data".to_owned()).into())
//     };
// }

// pub const MAGIC: u32 = 0xFC;

// #[derive(Debug, Clone, PartialEq, Eq)]
// pub struct Packet {
//     pub magic: u32,
//     pub data_len: u32,
//     pub payload: Vec<u8>,
// }

// #[derive(Debug, Clone, PartialEq, Eq)]
// pub enum Connect {
//     TCP(Option<Addr>),
//     UDP(Addr),
// }

// #[derive(Debug, Clone, PartialEq, Eq)]
// pub enum Bind {
//     Bind(Addr),
//     Failed(Addr, Vec<u8>),
// }

// #[derive(Debug, Clone, PartialEq, Eq)]
// pub enum Auth {
//     Auth(Vec<u8>),
//     NoAuth,
// }

// #[derive(Debug, Clone, PartialEq, Eq)]
// pub enum Message {
//     Bind(Bind),
//     Map(u32, Socket),
//     Connect(Connect, Auth),
// }

// trait BytesEx {
//     /// self.len() > expect
//     /// 自身剩余长度大于expect Ok 否则Err
//     fn fat(&self, expect: usize) -> Result<()>;
// }

// impl BytesEx for &[u8] {
//     fn fat(&self, expect: usize) -> Result<()> {
//         if self.len() < expect {
//             Err(Kind::Deserialize("invalid data".to_owned()).into())
//         } else {
//             Ok(())
//         }
//     }
// }

// trait GetMeta {
//     fn is(magic: u8) -> Result<()>
//     where
//         Self: Sized,
//     {
//         if magic == Self::get_magic() {
//             Ok(())
//         } else {
//             Err(Kind::Deserialize(format!("{:x} is not {}", magic, stringify!(Self))).into())
//         }
//     }

//     fn get_magic() -> u8
//     where
//         Self: Sized;

//     fn get_size(&self) -> usize;
// }

// impl GetMeta for Addr {
//     fn get_magic() -> u8
//     where
//         Self: Sized,
//     {
//         0x1
//     }

//     fn get_size(&self) -> usize {
//         2 + match self {
//             Addr::Socket(addr) => match addr {
//                 std::net::SocketAddr::V4(_) => 6,
//                 std::net::SocketAddr::V6(_) => 18,
//             },
//             Addr::Domain(domain, _) => {
//                 // u32 + data + port
//                 domain.as_bytes().len() + 6
//             }
//         }
//     }
// }

// impl GetMeta for Connect {
//     fn get_magic() -> u8
//     where
//         Self: Sized,
//     {
//         0x2
//     }

//     fn get_size(&self) -> usize {
//         2 + match self {
//             Connect::TCP(tcp) => tcp.get_size(),
//             Connect::UDP(udp) => udp.get_size(),
//         }
//     }
// }

// impl GetMeta for Bind {
//     fn get_magic() -> u8
//     where
//         Self: Sized,
//     {
//         0x3
//     }

//     fn get_size(&self) -> usize {
//         2 + match self {
//             Bind::Bind(bind) => bind.get_size(),
//             Bind::Failed(addr, err) => {
//                 // addr + u32 + data
//                 addr.get_size() + err.len() + 4
//             }
//         }
//     }
// }

// impl GetMeta for Auth {
//     fn get_magic() -> u8
//     where
//         Self: Sized,
//     {
//         0x4
//     }

//     fn get_size(&self) -> usize {
//         2 + match self {
//             Auth::Auth(auth) => auth.len(),
//             Auth::NoAuth => 0,
//         }
//     }
// }

// impl GetMeta for Message {
//     fn get_magic() -> u8
//     where
//         Self: Sized,
//     {
//         0x5
//     }

//     fn get_size(&self) -> usize {
//         2 + match self {
//             Message::Bind(bind) => bind.get_size(),
//             Message::Connect(connect, auth) => connect.get_size() + auth.get_size(),
//         }
//     }
// }

// impl TryFrom<&[u8]> for Message {
//     type Error = Error;

//     fn try_from(data: &[u8]) -> Result<Self> {
//         data.fat(2)?;

//         let mut cur = std::io::Cursor::new(data);

//         Self::is(cur.get_u8())?;

//         let magic = cur.get_u8();
//         let data = &data[2..];

//         match magic {
//             0x01 => Ok(Self::Bind(data.try_into()?)),
//             0x02 => {
//                 let addr = Connect::try_from(data)?;
//                 let auth = &data[addr.get_size()..];
//                 Ok(Self::Connect(addr, auth.try_into()?))
//             }
//             _ => invalid!(),
//         }
//     }
// }

// impl From<&Addr> for Vec<u8> {
//     fn from(addr: &Addr) -> Self {
//         let mut buf = Vec::new();

//         buf.put_u8(Addr::get_magic());

//         match addr {
//             Addr::Socket(SocketAddr::V4(addr)) => {
//                 buf.put_u8(0x1);
//                 buf.put_slice(&addr.ip().octets());
//                 buf.put_u16(addr.port())
//             }
//             Addr::Socket(SocketAddr::V6(addr)) => {
//                 buf.put_u8(0x2);
//                 buf.put_slice(&addr.ip().octets());
//                 buf.put_u16(addr.port())
//             }
//             Addr::Domain(domain, port) => {
//                 let domain = domain.as_bytes();
//                 buf.put_u8(0x3);
//                 buf.put_u32(domain.len() as u32);
//                 buf.put_slice(domain);
//                 buf.put_u16(*port);
//             }
//         }

//         buf
//     }
// }

// impl TryFrom<&[u8]> for Addr {
//     type Error = Error;
//     fn try_from(data: &[u8]) -> Result<Self> {
//         data.fat(2)?;

//         let mut cur = std::io::Cursor::new(data);

//         Self::is(cur.get_u8())?;

//         let magic = cur.get_u8();
//         let data = &data[2..];

//         match magic {
//             0x1 => {
//                 data.fat(6)?;

//                 Ok(Self::Socket(SocketAddr::new(
//                     Ipv4Addr::from(cur.get_u32()).into(),
//                     cur.get_u16(),
//                 )))
//             }
//             0x2 => {
//                 data.fat(18)?;

//                 Ok(Self::Socket(SocketAddr::new(
//                     Ipv6Addr::from(cur.get_u128()).into(),
//                     cur.get_u16(),
//                 )))
//             }
//             0x03 => {
//                 data.fat(4)?;

//                 let domain_len = cur.get_u32() as usize;
//                 let mut buf = Vec::with_capacity(domain_len);

//                 unsafe {
//                     buf.set_len(domain_len);
//                 }

//                 cur.read_exact(&mut buf)?;

//                 let domain = String::from_utf8(buf)?;

//                 data.fat(2)?;

//                 Ok(Self::Domain(domain, cur.get_u16()))
//             }
//             _ => {
//                 invalid!()
//             }
//         }
//     }
// }

// impl From<&Connect> for Vec<u8> {
//     fn from(addr: &Connect) -> Self {
//         let mut buf = Vec::new();

//         buf.put_u8(Connect::get_magic());

//         match addr {
//             Connect::TCP(addr) => {
//                 buf.put_u8(0x01);
//                 buf.put_slice(&Vec::from(addr));
//             }
//             Connect::UDP(addr) => {
//                 buf.put_u8(0x02);
//                 buf.put_slice(&Vec::from(addr));
//             }
//         }

//         buf
//     }
// }

// impl TryFrom<&[u8]> for Connect {
//     type Error = Error;

//     fn try_from(data: &[u8]) -> Result<Self> {
//         data.fat(2)?;

//         let mut cur = std::io::Cursor::new(data);

//         Self::is(cur.get_u8())?;

//         let magic = cur.get_u8();
//         let data = &data[2..];

//         match magic {
//             0x01 => Ok(Self::TCP(data.try_into()?)),
//             0x02 => Ok(Self::UDP(data.try_into()?)),
//             _ => invalid!(),
//         }
//     }
// }

// impl From<&Bind> for Vec<u8> {
//     fn from(bind: &Bind) -> Self {
//         let mut buf = Vec::new();

//         buf.put_u8(Bind::get_magic());

//         match bind {
//             Bind::Bind(addr) => {
//                 buf.put_u8(0x01);
//                 buf.put_slice(&Vec::from(addr))
//             }
//             Bind::Failed(addr, err) => {
//                 buf.put_u8(0x02);
//                 buf.put_slice(&Vec::from(addr));
//                 buf.put_u32(err.len() as u32);
//                 buf.put_slice(err)
//             }
//         }

//         buf
//     }
// }

// impl TryFrom<&[u8]> for Bind {
//     type Error = Error;
//     fn try_from(data: &[u8]) -> Result<Self> {
//         data.fat(2)?;

//         let mut cur = std::io::Cursor::new(data);

//         Self::is(cur.get_u8())?;

//         let magic = cur.get_u8();

//         let data = &data[2..];

//         match magic {
//             0x01 => Ok(Self::Bind(data.try_into()?)),
//             0x02 => {
//                 let addr = Addr::try_from(data)?;

//                 cur.advance(addr.get_size());

//                 let data = &data[addr.get_size()..];

//                 data.fat(4)?;

//                 let len = cur.get_u32() as usize;

//                 let data = &data[4..];

//                 data.fat(len)?;

//                 Ok(Self::Failed(addr, data[..len].to_vec()))
//             }
//             _ => invalid!(),
//         }
//     }
// }

// impl TryFrom<&[u8]> for Auth {
//     type Error = Error;

//     fn try_from(data: &[u8]) -> Result<Self> {
//         data.fat(2)?;

//         let mut cur = std::io::Cursor::new(data);

//         Self::is(cur.get_u8())?;

//         let magic = cur.get_u8();
//         let data = &data[2..];

//         match magic {
//             0x1 => {
//                 data.fat(4)?;
//                 let len = cur.get_u32() as usize;
//                 let data = &data[4..];
//                 data.fat(len)?;

//                 Ok(Self::Auth(data[..len].to_vec()))
//             }
//             0x2 => Ok(Self::NoAuth),
//             _ => invalid!(),
//         }
//     }
// }

// impl From<&Auth> for Vec<u8> {
//     fn from(auth: &Auth) -> Self {
//         let mut buf = Vec::new();

//         buf.put_u8(Auth::get_magic());

//         match auth {
//             Auth::Auth(auth) => {
//                 buf.put_u8(0x01);
//                 buf.put_u32(auth.len() as u32);
//                 buf.put_slice(auth);
//             }
//             Auth::NoAuth => {
//                 buf.put_u8(0x02);
//             }
//         }

//         buf
//     }
// }

// impl From<&Message> for Vec<u8> {
//     fn from(behavior: &Message) -> Self {
//         let mut buf = Vec::new();

//         buf.put_u8(Message::get_magic());

//         match behavior {
//             Message::Bind(bind) => {
//                 buf.put_u8(0x01);
//                 buf.put_slice(&Vec::from(bind));
//             }
//             Message::Connect(conn, auth) => {
//                 buf.put_u8(0x02);
//                 buf.put_slice(&Vec::from(conn));
//                 buf.put_slice(&Vec::from(auth));
//             }
//         }

//         buf
//     }
// }

// impl From<&Message> for Packet {
//     fn from(packet: &Message) -> Self {
//         let data = Vec::from(packet);
//         Packet {
//             magic: MAGIC,
//             data_len: data.len() as u32,
//             payload: data,
//         }
//     }
// }

// impl TryFrom<&Packet> for Message {
//     type Error = Error;

//     fn try_from(packet: &Packet) -> Result<Self> {
//         Message::try_from(packet.payload.as_slice())
//     }
// }
