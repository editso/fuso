use std::fmt::Display;

use bytes::{BufMut, BytesMut};
use serde::{Deserialize, Serialize};

use crate::{Addr, Socket};

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
    Bind(Addr),
    Failed(Addr, String),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum Auth {
    Auth(Vec<u8>),
    NoAuth,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum Message {
    Ping,
    MapError(u32, String),
    Bind(Bind),
    Map(u32, Socket),
    Connect(Connect, Auth),
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

pub trait ToPacket {
    fn to_packet_vec(self) -> Vec<u8>;
}

pub trait TryToMessage {
    fn try_message(self) -> crate::Result<Message>;
}

impl ToPacket for Message {
    fn to_packet_vec(self) -> Vec<u8> {
        let data = unsafe { bincode::serialize(&self).unwrap_unchecked() };
        super::make_packet(data).encode()
    }
}

impl TryToMessage for Packet {
    fn try_message(self) -> crate::Result<Message> {
        bincode::deserialize(&self.payload).map_err(Into::into)
    }
}

impl Display for Message{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#?}", self)
    }
}