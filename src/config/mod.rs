use std::{ops::Deref, sync::Arc};

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



#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RestartPolicy{
    Never,
    Always,
    Counter
}

impl Default for BootKind {
    fn default() -> Self {
        Self::Default
    }
}

impl Default for RestartPolicy{
    fn default() -> Self {
        RestartPolicy::Always
    }
}

pub struct Stateful<C>{
    pub conf: Arc<C>
}

impl<C> Stateful<C>{
    pub fn new(c: C) -> Self{
        Self { conf: Arc::new(c) }
    }
}

impl<C> Deref for Stateful<C>{
    type Target = Arc<C>;

    fn deref(&self) -> &Self::Target {
        &self.conf
    }
}

impl<C> Clone for Stateful<C>{
    fn clone(&self) -> Self {
        Stateful { conf: self.conf.clone() }
    }
}