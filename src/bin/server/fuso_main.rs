#![cfg(feature = "fus-log")]

mod fuso_toml;

use std::net::IpAddr;

use clap::Parser;

#[derive(Parser)]
pub struct FusoArgs {
    /// 监听的端口
    #[clap(short, long, default_value = "6722")]
    port: u16,
    /// 监听的地址
    #[clap(short, long, default_value = "0.0.0.0")]
    listen: IpAddr,
    /// 启用udp转发
    #[clap(long, default_value = "false")]
    enable_ufd: bool,
    /// 启用socks5
    #[clap(long, default_value = "false")]
    enable_socks: bool,
    /// webhook
    #[clap(long)]
    observer: Option<String>,
    #[clap(long, short)]
    config: Option<String>,
    /// 日志级别
    #[clap(long, default_value = "info")]
    log_level: log::LevelFilter,
    /// 发送心跳延时
    #[clap(long, default_value = "30")]
    heartbeat_delay: u64,
}

fn init_logger(log_level: log::LevelFilter) {
    let is_info_log = log_level.eq(&log::LevelFilter::Info);
    env_logger::builder()
        .filter_module("fuso", log_level)
        .default_format()
        .format_timestamp_millis()
        .format_target(!is_info_log)
        .init();
}

fn main() -> fuso::Result<()> {
    use std::time::Duration;

    use fuso::{
        observer::Executable, penetrate::PenetrateRsaAndAesHandshake, FusoExecutor,
        FusoUdpForwardProvider, FusoUdpServerProvider, Socket,
    };

    let args = FusoArgs::parse();

    init_logger(args.log_level);

    let config = fuso_toml::parse(args.config)?;

    // println!("{:#?}", config);

    fuso::block_on(async move {
        fuso::builder_server(Executable::new(args.observer, FusoExecutor))
            .using_handshake(PenetrateRsaAndAesHandshake::Server)
            .using_kcp(FusoUdpServerProvider, FusoExecutor)
            .using_penetrate()
            .heartbeat_timeout(Duration::from_secs(args.heartbeat_delay))
            .using_adapter()
            .using_direct()
            .using_socks()
            .using_udp_forward(FusoUdpForwardProvider)
            .build()
            .bind(Socket::tcp((args.listen, args.port)))
            .run()
            .await
    })
}
