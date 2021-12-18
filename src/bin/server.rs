use clap::Parser;

use fuso::aes::Aes;
use fuso_core::{
    auth::TokenAuth,
    core::{Fuso, GlobalConfig},
    handsnake::Handsnake,
    Forward, Security, Spawn, Xor,
};

use futures::{StreamExt, TryFutureExt};
use smol::Executor;

#[derive(Debug, Parser)]
#[clap(about, version)]
struct FusoArgs {
    /// 绑定地址
    #[clap(
        short = 'h',
        long = "host",
        default_value = "0.0.0.0",
        display_order = 1
    )]
    bind_host: String,

    /// 监听端口
    #[clap(short = 'p', long = "port", default_value = "9003", display_order = 2)]
    bind_port: u16,

    /// 认证类型
    #[clap(long = "auth", possible_values = ["token"])]
    fuso_auth_type: Option<String>,

    /// 认证密钥
    #[clap(long = "secret")]
    fuso_auth_secret: Option<String>,

    /// 日志级别
    #[clap(short, long, default_value = "info", possible_values = ["off", "error", "warn", "info", "debug", "trace"])]
    log_level: log::LevelFilter,
}

fn main() {
    let args = FusoArgs::parse();

    env_logger::builder()
        .filter_level(args.log_level)
        .format_timestamp_millis()
        .init();

    let bind_addr = format!("{}:{}", args.bind_host, args.bind_port);

    let core_future = async move {
       Fuso::builder()
            .use_auth(TokenAuth::new("my_token"))
            .use_default_handler()
            .use_default_strategy()
            .use_global_config(GlobalConfig::new(&bind_addr))
            .add_advice(fuso::rsa::server::RsaAdvice)
            .add_handsnake(Handsnake {
                prefix: "GET / HTTP/1.1".into(),
                suffix: "\r\n\r\n".into(),
                max_bytes: 1024,
                write: "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n".into(),
            })
            .add_cipher("xor", |secret|{
                if let Some(xor) = secret {
                    Ok(Xor::new(xor.parse().map_err(|_| {
                        fuso_core::Error::with_str("xor key error, expect 1-255")
                    })?))
                }else{
                    Err("xor secret is a required 1-255".into())
                }
            })
            .add_cipher("aes", |secret|{
                if let Some(secret) = secret{
                    Ok(Aes::try_from(secret)?)
                }else{
                    Err("aes secret is a required".into())
                }
            })
            .build()
            .map_ok(|fuso| {
                let ex = Executor::new();
                smol::block_on(ex.run(fuso.for_each(move |proxy| async move {

                    let cipher = proxy.try_get_cipher();

                    match cipher {
                        Ok(cipher) => {
                            let (proxy_src, proxy_dst, _) = proxy.split();
                      
                            match cipher {
                                Some(cipher) => {
                                    let proxy_dst = proxy_dst.cipher(cipher);
                                    proxy_dst.forward(proxy_src).detach();
                                },
                                None => {
                                    proxy_src.forward(proxy_dst).detach();
                                },
                            }

                        },
                        Err(e) => {
                            log::debug!("[proxy] cipher error {}", e);
                        },
                    }

                })));
            })
            .map_err(|e| async move {
                log::error!("{}", e);
            })
            .await
            .ok();
    };

    smol::block_on(core_future);
}
