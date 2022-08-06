use std::net::IpAddr;

use clap::ArgAction;
use clap::Parser;
use fuso::Socket;

#[derive(Parser)]
#[clap(author, version, about)]
struct FusoArgs {
    /// 是否启用 kcp, 默认不启用
    #[clap(long, default_value = "false", action = ArgAction::SetTrue, display_order=1)]
    kcp: bool,
    /// 映射名称
    #[clap(short, long, default_value = "anonymous", display_order = 1)]
    name: String,
    /// 启用socks
    #[clap(long, default_value = "false", action = ArgAction::SetTrue, display_order=2)]
    socks: bool,
    /// 映射成功,实际访问端口
    #[clap(
        long,
        visible_alias = "bind",
        visible_short_alias = 'b',
        default_value = "0",
        display_order = 9
    )]
    visit_bind_port: u16,
    /// 桥接监听地址
    #[clap(
        long,
        default_value = "127.0.0.1",
        visible_alias = "bl",
        display_order = 5
    )]
    bridge_listen: IpAddr,
    /// 桥接监听端口
    #[clap(long, visible_alias = "bp", display_order = 6)]
    bridge_port: Option<u16>,
    /// 服务端地址
    #[cfg(debug_assertions)]
    #[clap(default_value = "127.0.0.1")]
    server_host: String,
    /// 服务端地址
    #[cfg(not(debug_assertions))]
    server_host: String,
    /// 服务端端口
    #[clap(default_value = "6722")]
    server_port: u16,
    /// 转发地址
    #[clap(
        long,
        default_value = "127.0.0.1",
        visible_alias = "fh",
        display_order = 7
    )]
    forward_host: String,
    /// 转发端口
    #[clap(long, default_value = "80", visible_alias = "fp", display_order = 8)]
    forward_port: u16,
    /// 是否启用socks5 udp转发, 默认不启用
    #[clap(long, default_value = "false", visible_alias = "su", action = ArgAction::SetTrue, display_order=2)]
    socks_udp: bool,
    /// socks5账号
    #[clap(long, visible_alias = "s5u", display_order = 3)]
    socks_username: Option<String>,
    /// socks5密码
    #[clap(long, visible_alias = "s5p", display_order = 4)]
    socks_password: Option<String>,
    /// 最大等待读取时间
    #[clap(long, default_value = "5", display_order = 11)]
    maximum_rtime: u64,
    /// 最大等待写入时间
    #[clap(long, default_value = "5", display_order = 12)]
    maximum_wtime: u64,
    /// 最大等待建立连接时间
    #[clap(long, default_value = "10", display_order = 13)]
    maximum_wctime: u64,
    /// 发送心跳延时
    #[clap(long, default_value = "30", display_order = 14)]
    heartbeat_delay: u64,
}

#[cfg(feature = "fuso-rt-tokio")]
#[tokio::main]
async fn main() -> fuso::Result<()> {
    use std::time::Duration;

    use fuso::{penetrate::PenetrateRsaAndAesHandshake, TokioAccepter, TokioPenetrateConnector};

    let args = FusoArgs::parse();

    let fuso = fuso::builder_client_with_tokio()
        .using_handshake(PenetrateRsaAndAesHandshake::Client)
        .using_penetrate(
            Socket::tcp(args.visit_bind_port),
            Socket::tcp((args.forward_host, args.forward_port)),
        )
        .maximum_retries(None)
        .heartbeat_delay(Duration::from_secs(args.heartbeat_delay))
        .maximum_wait(Duration::from_secs(args.maximum_wctime))
        .set_name(args.name)
        .enable_kcp(args.kcp)
        .enable_socks5(args.socks)
        .enable_socks5_udp(args.socks_udp)
        .set_socks5_password(args.socks_password)
        .set_socks5_username(args.socks_username)
        .build(
            Socket::tcp((args.server_host, args.server_port)),
            TokioPenetrateConnector::new().await?,
        );

    let fuso = match args.bridge_port {
        None => fuso.run(),
        Some(port) => fuso
            .using_bridge(Socket::tcp((args.bridge_listen, port)), TokioAccepter)
            .run(),
    };

    fuso.await
}

#[cfg(feature = "fuso-web")]
#[tokio::main]
async fn main() {}

#[cfg(feature = "fuso-api")]
#[tokio::main]
async fn main() {}

#[cfg(feature = "fuso-rt-smol")]
fn main() -> fuso::Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .default_format()
        .format_module_path(false)
        .init();

    smol::block_on(async move {
        use fuso::SmolPenetrateConnector;

        fuso::builder_client_with_smol()
            .build(
                Socket::Tcp(8888.into()),
                PenetrateClientFactory {
                    connector_factory: Arc::new(SmolPenetrateConnector),
                    socket: {
                        (
                            Socket::Tcp(([0, 0, 0, 0], 9999).into()),
                            Socket::Tcp(([127, 0, 0, 1], 22).into()),
                        )
                    },
                },
            )
            .run()
            .await
    })
}
