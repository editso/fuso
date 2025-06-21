use std::{
    fmt::Display,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    ops::Deref,
    sync::Arc,
};

use serde::{Deserialize, Serialize};

use crate::error;

pub mod client;
pub mod server;

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]

pub enum Authentication {
    None,
    Secret(AuthWithSecret),
    Account(AuthWithAccount),
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct AuthWithSecret {
    secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct AuthWithAccount {
    username: String,
    password: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum BootKind {
    /// 为fork时, 表示开启一个子进程
    Fork,
    /// 默认为单进程模式
    Default,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Crypto {
    Aes,
    Rsa,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]

pub struct ListenMetadata {
    pub bind: IpAddr,
    pub port: u16,
}


#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Compress {
    Lz4,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct KeepAlive {
    interval: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum IP {
    V6,
    V4,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(into = "String")]
#[serde(try_from = "String")]
pub enum Expose {
    Kcp(IP, u16),
    Tcp(IP, u16)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RestartPolicy {
    Never,
    Always,
    Counter,
}

impl Default for BootKind {
    fn default() -> Self {
        Self::Default
    }
}

impl Default for RestartPolicy {
    fn default() -> Self {
        RestartPolicy::Always
    }
}

pub struct Stateful<C> {
    pub conf: Arc<C>,
}

impl<C> Stateful<C> {
    pub fn new(c: C) -> Self {
        Self { conf: Arc::new(c) }
    }
}

impl<C> Deref for Stateful<C> {
    type Target = Arc<C>;

    fn deref(&self) -> &Self::Target {
        &self.conf
    }
}

impl<C> Clone for Stateful<C> {
    fn clone(&self) -> Self {
        Stateful {
            conf: self.conf.clone(),
        }
    }
}

impl TryFrom<String> for Expose {
    type Error = error::FusoError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let lower = value.to_lowercase();

        let (port, ty) = match lower.split_once("/") {
            None => (value.as_str(), "tcp"),
            Some(r) => r,
        };

        let port = u16::from_str_radix(port, 10).map_err(|_| error::FusoError::InvalidPort)?;

        Ok({
            match ty {
                "kcp/v6" => Self::Kcp(IP::V6, port),
                "tcp/v6" => Self::Tcp(IP::V6, port),
                "kcp" | "kcp/v4" => Self::Kcp(IP::V4, port),
                "tcp" | "tcp/v4" => Self::Tcp(IP::V4, port),
                _ => return Err(error::FusoError::InvalidExposeType),
            }
        })
    }
}

impl From<Expose> for String {
    fn from(expose: Expose) -> Self {
        match expose {
            Expose::Kcp(ip, port) => format!("{port}/kcp/{ip}"),
            Expose::Tcp(ip, port) => format!("{port}/tcp/{ip}"),
        }
    }
}

impl Display for IP {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IP::V6 => write!(f, "v6"),
            IP::V4 => write!(f, "v4"),
        }
    }
}

impl IP {
    pub fn to_addr(&self, port: u16) -> SocketAddr {
        match self {
            IP::V4 => SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)),
            IP::V6 => SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, port, 0, 0)),
        }
    }
}

pub(crate) fn default_auth_timeout() -> u32 {
    60
}
