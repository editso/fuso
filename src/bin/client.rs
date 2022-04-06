use std::time::Duration;

use clap::Parser;

use fuso::{aes::Aes, rsa::client::RsaAdvice};
use fuso_core::{
    client::{Config, Fuso},
    handsnake::Handsnake,
    Forward, Security, Spawn, Xor,
};

use futures::{StreamExt, TryFutureExt};

use smol::Executor;

#[derive(Debug, Parser)]
#[clap(about, version)]
struct FusoArgs {
    /// 服务端主机地址
    #[clap(default_value = "127.0.0.1")]
    server_host: String,

    /// 服务端监听的端口
    #[clap(default_value = "9003")]
    server_port: u16,

    /// 转发主机地址
    #[clap(
        short = 'h',
        long,
        default_value = "127.0.0.1",
        takes_value = true,
        display_order = 1
    )]
    forward_host: String,

    /// 转发端口
    #[clap(
        short = 'p',
        long,
        default_value = "80",
        takes_value = true,
        display_order = 2
    )]
    forward_port: u16,

    /// 从公网访问的端口, 默认将自动分配
    #[clap(short = 'b', long, takes_value = true, display_order = 3)]
    visit_port: Option<u16>,

    /// 转发服务名称
    #[clap(short = 'n', long = "name", takes_value = true, display_order = 4)]
    forward_name: Option<String>,

    /// 加密类型
    #[clap(long, default_value = "aes", takes_value=true, possible_values = ["aes", "xor"], display_order = 5)]
    crypt_type: String,

    /// 加密所需密钥
    #[clap(long, default_value = "random", takes_value = true, display_order = 6)]
    crypt_secret: String,

    /// 前置握手方式, 默认不进行前置握手
    #[clap(long,  possible_values = ["websocket"], takes_value=true, display_order = 7)]
    handsnake: Option<String>,

    /// 本地桥接绑定地址
    #[clap(long, display_order = 8, takes_value = true)]
    bridge_host: Option<String>,

    /// 本地桥接监听端口
    #[clap(long, display_order = 9, takes_value = true)]
    bridge_port: Option<u16>,

    /// 转发类型
    #[clap(
        short = 't', 
        long, 
        default_value = "all", 
        display_order = 10, 
        takes_value=true, 
        possible_values = ["all", "socks5", "forward"],
    )]
    forward_type: String,

    /// socks5 连接密码, 默认不需要密码
    #[clap(long = "s5-pwd", display_order = 11, takes_value = true)]
    socks_password: Option<String>,

    /// 与服务端连接认证密码, 默认不需要密码
    #[clap(short = 'P', long = "fuso-pwd", takes_value = true)]
    fuso_password: Option<String>,

    /// 禁用rsa加密
    #[clap(long, default_value = "false", takes_value = true, possible_values = ["true", "false"])]
    disable_rsa: String,

    /// 日志级别
    #[clap(
        short, 
        long, 
        default_value = "info", 
        takes_value=true, 
        display_order = 11,
        possible_values = ["off", "error", "warn", "info", "debug", "trace"], 
    )]
    log_level: log::LevelFilter,
}

fn main() {
    let mut args = FusoArgs::parse();

    env_logger::builder().filter_level(args.log_level).init();

    let server_addr = format!("{}:{}", args.server_host, args.server_port);

    let handsnake = args.handsnake.map_or(None, |handsnake|{
        match handsnake.as_str() {
            "websocket" => {
               Some(Handsnake{
                prefix: "HTTP/1.1 101".into(),
                suffix: "\r\n\r\n".into(),
                max_bytes: 1024,
                write: format!("GET / HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n", server_addr),
            }) 
            },
            _ => None,
        }
    });

    match args.crypt_type.as_str() {
        "aes" => {
            let aes = Aes::try_with(args.crypt_secret.as_str()).expect("aes key error");
            args.crypt_secret = aes.to_hex_string();
        }
        "xor" => {
            Xor::new(args.crypt_secret.parse().expect("xor key error 1-255"));
        }
        _ => panic!(),
    };

    let crypt_type = args.crypt_type.clone();

    let bridge_addr = {
        match (args.bridge_port, args.bridge_host) {
            (Some(port), addr) => Some(format!("{}:{}", addr.unwrap_or("0.0.0.0".into()), port)),
            _ => None,
        }
    };

    let forward_addr = format!("{}:{}", args.forward_host, args.forward_port);

    let disable_rsa = match args.disable_rsa.as_str() {
        "true" => true,
        "false" => false,
        _ => unreachable!(),
    };

    let config = Config {
        handsnake: handsnake,
        crypt_type: Some(args.crypt_type),
        visit_port: args.visit_port,
        bridge_addr: bridge_addr,
        server_addr: server_addr,
        forward_name: args.forward_name,
        forward_addr: forward_addr,
        socks_passwd: args.socks_password,
        forward_type: Some(args.forward_type),
        crypt_secret: Some(args.crypt_secret.clone()),
    };

    let core_future = async move {
        loop {
            let crypt_type = crypt_type.clone();
            let fuso = Fuso::builder().use_config(config.clone());
            if disable_rsa {
                log::warn!("rsa has been disabled and may cause security issues");
                fuso
            } else {
                fuso.add_advice(RsaAdvice)
            }
            .set_cipher(move |secret| match (crypt_type.as_str(), secret) {
                ("xor", Some(key)) => Ok(Some(Box::new(Xor::new(
                    key.parse().expect("xor key error 1-255"),
                )))),
                ("aes", Some(key)) => Ok(Some(Box::new(Aes::try_from(key)?))),
                _ => Ok(None),
            })
            .build()
            .map_ok(|fuso| {
                let ex = Executor::new();
                smol::block_on(ex.run(fuso.for_each(|proxy| async move {
                    proxy
                        .join()
                        .map_ok(|(from, to, cx)| {
                            async move {
                                let cipher = cx.get_cipher().unwrap();

                                let status = match cipher {
                                    Some(cipher) => {
                                        let from = from.cipher(cipher);
                                        from.forward(to).await
                                    }
                                    None => from.forward(to).await,
                                };

                                if let Err(e) = status {
                                    log::debug!("[fuc] Forwarding failed {}", e);
                                }
                            }
                            .detach()
                        })
                        .map_err(|e| log::warn!("{}", e))
                        .await
                        .ok();
                })))
            })
            .map_err(|e| log::warn!("{}", e))
            .await
            .ok();

            smol::Timer::after(Duration::from_secs(2)).await;

            log::debug!("[fuc] Try to reconnect");
        }
    };

    smol::block_on(core_future);
}
