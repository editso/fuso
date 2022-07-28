pub mod udp;

#[cfg(feature = "fuso-kcp")]
pub mod kcp;

#[cfg(feature = "fuso-quic")]
pub mod quic;

#[cfg(feature = "fuso-socks5")]
pub mod socks;

pub mod tun;

#[cfg(feature = "fuso-proxy")]
pub mod proxy;

pub mod penetrate;
