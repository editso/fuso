use std::fmt::Display;

use bytes::{BufMut, BytesMut};
use serde::{Deserialize, Serialize};

use crate::{Addr, Address, Socket};

use super::make_packet;

pub const MAGIC: u32 = 0xFC;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub magic: u32,
    pub data_len: u32,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum Connect {
    TCP(Option<Addr>),
    UDP(Addr),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum Bind {
    Setup(Socket, Socket),
    Success(Address, Address),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum Auth {
    Auth(Vec<u8>),
    NoAuth,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum Poto {
    Ping,
    Close,
    MapError(u32, String),
    Bind(Bind),
    Map(u32, Socket),
    Connect(Connect, Auth),
    Forward(Addr),
}

impl Packet {
    pub fn encode(self) -> Vec<u8> {
        let mut packet = BytesMut::new();
        packet.put_u32(self.magic);
        packet.put_u32_le(self.data_len);
        packet.put_slice(&self.payload);
        packet.to_vec()
    }
}

pub trait ToBytes {
    fn bytes(self) -> Vec<u8>;
}

pub trait IntoPacket {
    fn into_packet(self) -> Packet;
}

pub trait TryToPoto {
    fn try_poto(self) -> crate::Result<Poto>;
}

impl<T> IntoPacket for T
where
    T: Serialize,
{
    fn into_packet(self) -> Packet {
        let data = unsafe { bincode::serialize(&self).unwrap_unchecked() };
        make_packet(data)
    }
}

impl<T> ToBytes for T
where
    T: Serialize,
{
    fn bytes(self) -> Vec<u8> {
        let data = unsafe { bincode::serialize(&self).unwrap_unchecked() };
        super::make_packet(data).encode()
    }
}

impl TryToPoto for Packet {
    fn try_poto(self) -> crate::Result<Poto> {
        bincode::deserialize(&self.payload).map_err(Into::into)
    }
}

impl Display for Poto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#?}", self)
    }
}
