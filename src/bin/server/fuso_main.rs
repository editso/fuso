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
    /// 日志级别
    #[cfg(debug_assertions)]
    #[cfg(feature = "fus-log")]
    #[clap(long, default_value = "debug")]
    log_level: log::LevelFilter,
    /// 日志级别
    #[cfg(feature = "fus-log")]
    #[cfg(not(debug_assertions))]
    #[clap(long, default_value = "info")]
    log_level: log::LevelFilter,
    /// 发送心跳延时
    #[clap(long, default_value = "30")]
    heartbeat_delay: u64,
}

#[cfg(feature = "fus-log")]
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
    use fuso::{
        observer::Executable, penetrate::PenetrateRsaAndAesHandshake, FusoExecutor,
        FusoUdpServerProvider, Socket, UdpForwardProvider,
    };
    use std::time::Duration;

    let args = FusoArgs::parse();

    #[cfg(feature = "fus-log")]
    init_logger(args.log_level);

    fuso::block_on(async move {
        fuso::builder_server(Executable::new(args.observer, FusoExecutor))
            .using_handshake(PenetrateRsaAndAesHandshake::Server)
            .using_kcp(FusoUdpServerProvider, FusoExecutor)
            .using_penetrate()
            .heartbeat_timeout(Duration::from_secs(args.heartbeat_delay))
            .using_adapter()
            .using_direct()
            .using_socks()
            .using_udp_forward(UdpForwardProvider)
            .build()
            .bind(Socket::tcp((args.listen, args.port)))
            .run()
            .await
    })
}
