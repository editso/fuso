mod fuso_toml;

use std::{net::IpAddr, sync::Arc};

use clap::Parser;

use fuso::{
    penetrate::PenetrateWebhook,
    webhook::{hyper::HTTPWebhook, telegram::Telegram},
    Webhook,
};
use fuso_toml::Config;

use fuso::{
    penetrate::PenetrateRsaAndAesHandshake, FusoExecutor, FusoUdpForwardProvider,
    FusoUdpServerProvider, Socket,
};

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
        .default_format()
        .filter_module("fus", log_level)
        .filter_module("fuso", log_level)
        .format_timestamp_millis()
        .format_target(!is_info_log)
        .init();
}

fn main() -> fuso::Result<()> {
    let args = FusoArgs::parse();
    let config = Arc::new(fuso_toml::parse(args.config)?);

    init_logger(config.global.log_level);

    log::trace!("config = {:#?}", config);

    fuso::block_on(async move {
        match config.global.webhook {
            Some(ref webhook) => match webhook {
                fuso_toml::Webhook::Http {
                    server,
                    use_ssl,
                    method,
                    headers,
                    format_mode,
                } => {
                    let http = HTTPWebhook {
                        server: server.clone(),
                        format: format_mode.clone(),
                        method: method.clone(),
                        enable_ssl: use_ssl.clone(),
                        executor: FusoExecutor,
                        headers: headers
                            .iter()
                            .map(|(k, v)| (k.to_owned(), v.to_owned()))
                            .collect(),
                    };

                    run_async_main(config, http).await
                }
                fuso_toml::Webhook::Telegram {
                    server,
                    chat_id,
                    bot_token,
                } => {
                    let telegram = Telegram::new(
                        bot_token.clone(),
                        chat_id.clone(),
                        fuso::FusoExecutor,
                        Some(server.clone()),
                    );
                    run_async_main(config, telegram).await
                }
            },
            None => run_async_main(config, ()).await,
        }
    })
}

async fn run_async_main<W>(config: Arc<Config>, webhook: W) -> fuso::Result<()>
where
    W: Webhook + PenetrateWebhook + Send + Sync + 'static,
{
    let fuso_builder = fuso::builder_server(webhook);

    match config.global.feature {
        fuso_toml::Feature::Tun => todo!(),
        fuso_toml::Feature::Proxy => todo!(),
        fuso_toml::Feature::Penetrate => {
            let mut fuso_builder = fuso_builder
                .using_handshake(PenetrateRsaAndAesHandshake::Server)
                .using_kcp(FusoUdpServerProvider, FusoExecutor)
                .using_penetrate()
                .using_selector();

            for feature in config.penetrate.features.iter() {
                fuso_builder = match feature {
                    fuso_toml::PenetrateFuture::Proxy => fuso_builder.using_direct(),
                    fuso_toml::PenetrateFuture::Socks { udp_forward: false } => {
                        fuso_builder.using_socks().simple()
                    }
                    fuso_toml::PenetrateFuture::Socks { udp_forward: true } => fuso_builder
                        .using_socks()
                        .using_udp_forward(FusoUdpForwardProvider),
                    fuso_toml::PenetrateFuture::PProxy => unimplemented!(),
                    fuso_toml::PenetrateFuture::Websocket => unimplemented!(),
                }
            }

            fuso_builder
                .build()
                .bind(Socket::tcp((config.global.bind, config.global.port)))
                .run()
                .await
        }
    }
}
