use serde::{Deserialize, Serialize};

pub mod client;
pub mod server;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]

pub enum Authentication {
    None,
    Secret(AuthWithSecret),
    Account(AuthWithAccount),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthWithSecret {
    secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    Rsa
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

impl Default for BootKind {
    fn default() -> Self {
        Self::Default
    }
}
