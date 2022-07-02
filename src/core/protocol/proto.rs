use std::{
    io::Read,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
};

use bytes::{Buf, BufMut};

use crate::{Addr, Error, Kind, Result, Socket};

macro_rules! invalid {
    () => {
        Err(Kind::Deserialize("invalid data".to_owned()).into())
    };
}

pub const MAGIC: u32 = 0xFC;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub magic: u32,
    pub data_len: u32,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connect {
    TCP(Option<Addr>),
    UDP(Addr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bind {
    Bind(Addr),
    Failed(Addr, Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auth {
    Auth(Vec<u8>),
    NoAuth,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Bind(Bind),
    Map(u32, Socket),
    Connect(Connect, Auth),
}

pub trait ToVec {
    fn to_vec(self) -> Vec<u8>;
}

impl ToVec for Message{
    fn to_vec(self) -> Vec<u8> {
        unimplemented!()
    }
}