use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

use crate::config::client::Addr;

#[derive(Serialize, Deserialize)]
pub enum Fragment {
    Ping,
    Udp(SocketAddr, Vec<u8>),
    UdpForward {
        saddr: SocketAddr,
        daddr: Addr,
        dport: u16,
        data: Vec<u8>,
    },
}
