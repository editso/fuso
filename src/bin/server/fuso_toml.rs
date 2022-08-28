#![cfg(feature = "fuso-toml")]

use std::{collections::HashMap, net::IpAddr};

use fuso::webhook;
use serde::{Deserialize, Serialize};

#[inline]
fn default_bind() -> IpAddr {
    [0, 0, 0, 0].into()
}

#[inline]
fn default_port() -> u16 {
    6722
}

#[inline]
fn default_web_port() -> u16 {
    6780
}

#[inline]
fn default_telegram_server() -> String {
    String::from("api.telegram.org")
}

#[inline]
fn default_log_level() -> log::LevelFilter {
    log::LevelFilter::Info
}

fn default_penetrate_futures() -> Vec<PenetrateFuture> {
    vec![
        PenetrateFuture::Socks { udp_forward: true },
        PenetrateFuture::Proxy,
    ]
}

fn default_web_context() -> String {
    String::from("/")
}

#[derive(Debug, Deserialize, Serialize)]
pub enum Feature {
    Tun,
    Proxy,
    Penetrate,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "params")]
pub enum HandshakePolicy {
    Anyone,
    Token(String),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Global {
    #[serde(default = "default_bind")]
    pub bind: IpAddr,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "Default::default")]
    pub feature: Feature,
    #[serde(default = "Default::default")]
    pub handshake: HandshakePolicy,
    #[serde(default = "default_log_level")]
    pub log_level: log::LevelFilter,
    #[serde(default = "Default::default")]
    pub webhook: Option<Webhook>,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum PenetrateFuture {
    // socks5
    Socks { udp_forward: bool },
    // proxy
    Proxy,
    // proxy protocol
    PProxy,
    // websocket
    Websocket,
    // udp forward
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Dashboard {
    #[serde(default = "default_bind")]
    bind: IpAddr,
    #[serde(default = "default_web_port")]
    port: u16,
    #[serde(default = "default_web_context")]
    context: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum WebhookFormat {
    Html,
    Json,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum WebhookMethod {
    Get,
    Post,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "params")]
pub enum Webhook {
    Http {
        server: String,
        #[serde(default = "Default::default")]
        use_ssl: bool,
        method: webhook::hyper::Method,
        #[serde(default = "Default::default")]
        headers: HashMap<String, String>,
        #[serde(default = "Default::default")]
        format_mode: webhook::hyper::Format,
    },
    Telegram {
        #[serde(default = "default_telegram_server")]
        server: String,
        chat_id: String,
        bot_token: String,
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Penetrate {
    #[serde(default = "default_penetrate_futures")]
    pub features: Vec<PenetrateFuture>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "Default::default")]
    pub global: Global,
    #[serde(default = "Default::default")]
    pub penetrate: Penetrate,
    #[serde(default = "Default::default")]
    pub dashboard: Option<Dashboard>,
}

impl Default for Feature {
    fn default() -> Self {
        Self::Penetrate
    }
}

impl Default for Global {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            port: default_port(),
            feature: Default::default(),
            handshake: Default::default(),
            log_level: default_log_level(),
            webhook: Default::default(),
        }
    }
}

impl Default for HandshakePolicy {
    fn default() -> Self {
        Self::Anyone
    }
}

impl Default for Penetrate {
    fn default() -> Self {
        Self {
            features: default_penetrate_futures(),
        }
    }
}

impl Default for WebhookFormat {
    fn default() -> Self {
        WebhookFormat::Json
    }
}

impl Default for WebhookMethod {
    fn default() -> Self {
        WebhookMethod::Get
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            global: Default::default(),
            penetrate: Default::default(),
            dashboard: Default::default(),
        }
    }
}

pub fn parse<S>(config: Option<S>) -> fuso::Result<Config>
where
    S: AsRef<str> + 'static,
{
    match config {
        None => Ok(Config::default()),
        Some(config) => Ok(toml::from_slice(&std::fs::read(config.as_ref())?)?),
    }
}
