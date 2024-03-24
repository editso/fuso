use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr},
    ops::Deref,
};

use serde::{Deserialize, Serialize};

use super::{Authentication, Compress, Crypto};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// 加密方式
    #[serde(default = "Default::default")]
    pub crypto: HashSet<Crypto>,
    /// 压缩方式
    #[serde(default = "Default::default")]
    pub compress: HashSet<Compress>,
    #[serde(rename = "listen")]
    pub listens: HashSet<Listen>,
    pub manager: Option<Manager>,
    pub features: HashSet<Feature>,
    #[serde(rename = "auth")]
    pub authentication: Authentication,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manager {
    pub bind: IpAddr,
    pub port: u16,
    pub context: String,
    #[serde(rename = "auth")]
    pub authentication: Authentication,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Feature {
    Kcp,
    Proxy,
    Socks5,
    Forward,
    MultiProcess,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Listen {
    Kcp(KcpListenMetadata),
    Tcp(ListenMetadata),
    Proxy(ListenMetadata),
    Tunnel(ListenMetadata),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]

pub struct ListenMetadata {
    pub bind: IpAddr,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct KcpListenMetadata {
    #[serde(flatten)]
    pub metadata: ListenMetadata,
    // config: kcp_rust::Config
}

impl ListenMetadata {
    pub fn as_socket_addr(self) -> SocketAddr {
        SocketAddr::new(self.bind, self.port)
    }
}

impl From<ListenMetadata> for SocketAddr {
    fn from(value: ListenMetadata) -> Self {
        value.as_socket_addr()
    }
}

impl Deref for KcpListenMetadata {
    type Target = ListenMetadata;
    fn deref(&self) -> &Self::Target {
        &self.metadata
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            listens: Default::default(),
            manager: None,
            features: HashSet::new(),
            authentication: Authentication::None,
            crypto: Default::default(),
            compress: Default::default(),
        }
    }
}

#[cfg(test)]
#[cfg(feature = "fuso-toml")]
mod tests {
    use std::collections::HashSet;

    use crate::config::Authentication;

    use super::Config;

    #[test]
    fn test_server_config() -> std::io::Result<()> {
        let config = std::fs::read("config/server.toml")?;

        println!("{}", String::from_utf8_lossy(&config));

        let a = toml::from_str::<Config>(&String::from_utf8(config).unwrap()).unwrap();

        println!("{:#?}", a);

        Ok(())
    }
}
