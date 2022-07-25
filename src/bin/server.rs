use std::{net::IpAddr, str::FromStr};

use clap::Parser;

pub enum Kind {
    Proxy,
    Forward,
}

#[derive(Parser)]
pub struct FusoArgs {
    /// 监听的端口
    #[clap(short, long, default_value = "6722")]
    port: u16,
    /// 监听的地址
    #[clap(short, long, default_value = "0.0.0.0")]
    listen: IpAddr,
    /// 配置文件
    #[clap(short, long)]
    config: Option<String>,
    #[clap(short, long, default_value = "forward")]
    kind: Kind,
    /// 启用udp转发
    #[clap(long, default_value = "false")]
    enable_ufd: bool,
    /// 启用socks5
    #[clap(long, default_value = "false")]
    enable_socks: bool,
    /// 日志级别
    #[cfg(debug_assertions)]
    #[clap(long, default_value = "debug")]
    log_level: log::LevelFilter,
    /// 日志级别
    #[cfg(not(debug_assertions))]
    #[clap(long, default_value = "info")]
    log_level: log::LevelFilter,
    /// 发送心跳延时
    #[clap(long, default_value = "30")]
    heartbeat_delay: u64,
    /// 最大等待读取时间
    #[clap(long, default_value = "5")]
    maximum_rtime: u64,
    /// 最大等待写入时间
    #[clap(long, default_value = "5")]
    maximum_wtime: u64,
    /// 最大等待建立连接时间
    #[clap(long, default_value = "10")]
    maximum_wctime: u64,
}

fn init_logger(log_level: log::LevelFilter) {
    let is_info_log = log_level.eq(&log::LevelFilter::Info);
    env_logger::builder()
        .filter_module("fuso", log_level)
        .default_format()
        .format_timestamp_millis()
        .format_target({
            if is_info_log {
                false
            } else {
                cfg!(debug_assert)
            }
        })
        .init();
}

#[cfg(feature = "fuso-rt-tokio")]
#[tokio::main]
async fn main() -> fuso::Result<()> {
    use fuso::{Socket, TokioExecutor, TokioUdpServerProvider, UdpForwardProvider};
    use std::time::Duration;

    let args = FusoArgs::parse();

    init_logger(args.log_level);   

    fuso::builder_server_with_tokio()
        .with_kcp_accepter(TokioUdpServerProvider, TokioExecutor)
        .with_penetrate()
        .max_wait_time(Duration::from_secs(args.maximum_wctime))
        .heartbeat_timeout(Duration::from_secs(args.heartbeat_delay))
        .with_adapter_mode()
        .with_normal_unpacker()
        .with_socks_unpacker()
        .with_udp_forward(UdpForwardProvider)
        .build()
        .bind(Socket::tcp((args.listen, args.port)))
        .run()
        .await
        .expect("server start failed");

    Ok(())
}

#[cfg(feature = "fuso-web")]
#[tokio::main]
async fn main() {}

#[cfg(feature = "fuso-api")]
#[tokio::main]
async fn main() {}

#[cfg(feature = "fuso-rt-smol")]
fn main() -> fuso::Result<()> {
    use fuso::{Handshake, Socket};

    env_logger::builder()
        .filter_module("fuso", log::LevelFilter::Trace)
        .default_format()
        .format_module_path(false)
        .init();

    smol::block_on(async move {
        fuso::builder_server_with_smol()
            .with_handshake(Handshake)
            .with_penetrate()
            .with_adapter_mode()
            .use_normal()
            .use_socks()
            .build()
            .bind(Socket::Tcp(([0, 0, 0, 0], 8888).into()))
            .run()
            .await
    })
}

impl FromStr for Kind {
    type Err = &'static str;

    fn from_str(kind: &str) -> Result<Self, Self::Err> {
        Ok(match kind {
            "proxy" => Self::Proxy,
            "forward" => Self::Forward,
            _ => return Err("kind error"),
        })
    }
}
