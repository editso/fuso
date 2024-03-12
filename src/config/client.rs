use std::{
    collections::{HashMap, HashSet},
    io,
    net::IpAddr,
};

use serde::{Deserialize, Serialize};

use super::{Authentication, BootKind, Compress, Crypto, KeepAlive};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub server: Server,
    pub features: Vec<Feature>,
    #[serde(flatten)]
    pub services: HashMap<String, Service>,
    #[serde(default = "Default::default")]
    pub default_crypto: Vec<Crypto>,
    #[serde(default = "Default::default")]
    pub default_compress: Vec<Compress>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Feature {
    Kcp,
    Socks5,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Server {
    pub addr: ServerAddr,
    pub ports: Vec<u16>,
    pub retries: i32,
    #[serde(default = "Default::default")]
    pub crypto: Vec<Crypto>,
    #[serde(default = "Default::default")]
    pub compress: Vec<Compress>,
    #[serde(rename = "auth")]
    pub authentication: Authentication,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ServerAddr {
    WithIpAddr(Vec<IpAddr>),
    WithDomain(Vec<String>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Service {
    /// 代理
    Proxy(WithProxyService),
    Bridge(WithBridgeService),
    /// 端口转发
    Forward(WithForwardService),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WithForwardService {
    /// 服务器运行方式
    #[serde(default = "Default::default")]
    pub boot: BootKind,
    /// 转发到的目标地址
    pub target: FinalTarget,
    /// 对于某些长连接的请求，保持会话
    pub keep_alive: Option<KeepAlive>,
    /// 服务端对外暴露的端口， 可以填写多个, 确保端口没有被占用
    /// 如果没有填写，那么随机分配，这时请确保防火墙中该随机端口被允许访问
    #[serde(default = "Default::default")]
    pub exposes: Vec<u16>,
    /// 与服务器建立连接时的端口
    /// 该字段是可选的, 如果没有填写, 将会使用`exposes`字段中的端口来建立连接
    /// 如果填写了0, 那么会随机分配一个端口，这时请确保防火墙中该随机端口被允许访问
    pub channel: Option<u16>,
    /// 加密方式
    #[serde(default = "Default::default")]
    pub crypto: HashSet<Crypto>,
    /// 压缩方式
    #[serde(default = "Default::default")]
    pub compress: HashSet<Compress>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WithBridgeService {
    pub bind: IpAddr,
    pub port: u16,
    #[serde(rename = "auth")]
    pub authentication: Authentication,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WithProxyService {
    /// 监听地址
    pub bind: IpAddr,
    /// 监听端口
    pub port: u16,
    /// 启动方式
    #[serde(default = "Default::default")]
    pub boot: BootKind,
    #[serde(default = "Default::default")]
    pub rewrite: Option<Rewrite>,
    /// 转发到的目标地址
    #[serde(default = "Default::default")]
    pub target: FinalTarget,
    /// 加密方式
    #[serde(default = "Default::default")]
    pub crypto: HashSet<Crypto>,
    /// 压缩方式
    #[serde(default = "Default::default")]
    pub compress: HashSet<Compress>,
    /// 对于某些长连接的请求，保持会话
    pub keep_alive: Option<KeepAlive>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "with", rename_all = "lowercase")]
pub enum Rewrite {
    #[serde(rename = "http_header")]
    HttpHeader(WithHttpHeaderRewrite),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WithHttpHeaderRewrite {
    #[serde(flatten)]
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FinalTarget {
    /// 静态地址
    Static { addr: ServerAddr, port: u16 },
    /// 当该地址是一个动态地址时, 代理模式将会变为socks5
    Dynamic,
}

impl Default for FinalTarget {
    fn default() -> Self {
        Self::Dynamic
    }
}


impl ServerAddr {
    async fn connect(&self, port: u16) -> io::Result<()> {
        match self {
            ServerAddr::WithIpAddr(addr) => {
                unimplemented!()
            }
            ServerAddr::WithDomain(domain) => {
                unimplemented!()
            }
        }
    }
}


#[cfg(test)]
#[cfg(feature = "fuso-toml")]
mod tests {

    use super::Config;

    #[test]
    fn test_client_config() -> std::io::Result<()> {
        let config = std::fs::read("config/client.toml")?;

        println!("{}", String::from_utf8_lossy(&config));

        let a = toml::from_str::<Config>(&String::from_utf8(config).unwrap()).unwrap();

        // let a = a.server.addr.connect(80).await;

        // let a = a.services.get("").unwrap();

        println!("{:#?}", a);

        Ok(())
    }
}