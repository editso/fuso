use std::{ time::Duration};

use clap::{Parser};

use fuso_core::{
    cipher::{Security, Xor},
    client::{Fuso, Config},
    Forward, Spawn, handsnake::Handsnake,
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
        long,
        default_value = "127.0.0.1",
        takes_value = true,
        display_order = 1
    )]
    forward_host: String,

    /// 转发端口
    #[clap(long, default_value = "80", takes_value = true, display_order = 2)]
    forward_port: String,

    /// 从公网访问的端口, 默认将自动分配
    #[clap(short = 'b', long, takes_value = true, display_order = 3)]
    visit_port: Option<u16>,

    /// 转发服务名称
    #[clap(short='n', long = "name", takes_value = true, display_order = 4)]
    forward_name: Option<String>,

    /// 加密类型
    #[clap(long, default_value = "xor", takes_value=true, possible_values = ["xor"], display_order = 5)]
    crypt_type: String,

    /// 加密所需密钥
    #[clap(long, default_value = "27", takes_value = true, display_order = 6)]
    crypt_secret: String,

    /// 握手方式, 默认不进行握手
    #[clap(long,  possible_values = ["websocket"], takes_value=true, display_order = 7)]
    handsnake: Option<String>,

    /// 本地桥接绑定地址
    #[clap(long, display_order = 8, takes_value=true)]
    bridge_host: Option<String>,

    /// 本地桥接监听端口
    #[clap(long, display_order = 9, takes_value=true)]
    bridge_port: Option<u16>,

    /// 转发类型
    #[clap(short = 't', long, default_value = "all", display_order = 10, takes_value=true, possible_values = ["all", "socks5", "forward"])]
    forward_type: String,

    /// socks5 连接密码, 默认不需要密码
    #[clap(long = "s5passwd", takes_value=true)]
    socks_password: Option<String>,  
    
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
    let args = FusoArgs::parse();
    
    env_logger::builder()
    .filter_level(args.log_level)
    .format_timestamp_millis()
    .init();

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
    
    let config = Config{
        forward_name: args.forward_name,
        server_addr: server_addr,
        visit_port: args.visit_port,
        bridge_addr: Some(format!("{}:{}", args.bridge_host.unwrap_or("0.0.0.0".into()), args.bridge_port.unwrap_or(0))),
        forward_addr: format!("{}:{}", args.forward_host, args.forward_port),
        handsnake: handsnake,
        socks_passwd: args.socks_password,
        forward_type: Some(args.forward_type),
        crypt_type: Some(args.crypt_type),
        crypt_secret: Some(args.crypt_secret),
    };

    let future = async move{
        loop {
            Fuso::with_config(config.clone())
            .map_ok(|fuso| {
                let ex = Executor::new();
                smol::block_on(ex.run(fuso.for_each(|reactor| async move {
                    
                    reactor
                        .join()
                        .map_ok(|(from, to, cfg)| {
                            async move {
                                
                                let cipher = Xor::new(27);

                                let from = from.cipher(cipher).await;

                                if let Err(e) = from.forward(to).await {
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

    smol::block_on(future);
}
