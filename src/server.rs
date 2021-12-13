use std::sync::Arc;

use clap::Parser;

use fuso_core::{
    cipher::{Security, Xor},
    core::{FusoConfig, Fuso},
    dispatch::State,
    handsnake::{Handsnake, HandsnakeEx},
    packet::{Action, Addr},
    Forward, FusoPacket, Spawn,
};

use fuso_socks::{AsyncUdpForward, Socks, Socks5Ex, PasswordAuth};
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

    /// 日志级别
    #[clap(short, long, default_value = "info", possible_values = ["off", "error", "warn", "info", "debug", "trace"])]
    log_level: log::LevelFilter,
}


fn main() {
    let args = FusoArgs::parse();

    let xor_secret: u8 = 27;

    env_logger::builder()
        .filter_level(args.log_level)
        .format_timestamp_millis()
        .init();

    let future = async move {
        Fuso::builder()
            .with_config(FusoConfig {
                debug: false,
                bind_addr: format!("{}:{}", args.bind_host, args.bind_port),
            })
            .add_handsnake(Handsnake {
                prefix: "GET / HTTP/1.1".into(),
                suffix: "\r\n\r\n".into(),
                max_bytes: 1024,
                write: "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n".into(),
            })
            .chain_handler(|chain| {
                chain
                    .next(|mut tcp, cx| async move {
                        
                        for handsnake in cx.handsnakes.iter() {
                            let _ = tcp.begin().await;

                            tcp.handsnake(handsnake).await?;

                            let _ = tcp.back().await;
                        }

                        Ok(State::Next)
                    })
                    .next(|mut tcp, cx| async move {
                        
                        let action: Action = tcp.recv().await?.try_into()?;
                        let _ = tcp.begin().await;
                        
                        match action {
                            Action::TcpBind(cfg) => {
                                match cx.spawn(tcp.clone(), cfg).await {
                                    Ok(conv) => {
                                        log::debug!(
                                            "[fuso] accept conv={}, addr={}",
                                            conv,
                                            tcp.peer_addr().unwrap(),
                                        );
                                        Ok(State::Accept(()))
                                    }
                                    Err(e) => {
                                        log::warn!(
                                            "[fuso] Failed to open the mapping {}",
                                            e.to_string()
                                        );
                                        let _ = tcp.send(Action::Err(e.to_string()).into()).await?;
                                        Ok(State::Accept(()))
                                    }
                                }
                            }
                            Action::UdpBind(conv) => {
            
                                cx.route(conv, action, tcp).await?;
                                Ok(State::Accept(()))
                            }
                            Action::Connect(conv, _) => {
                        
                                cx.route(conv, action, tcp).await?;
                                Ok(State::Accept(()))
                            }
                            _ => {
                                let _ = tcp.back().await;
                                Ok(State::Next)
                            }
                        }
                    })
            })
            .chain_strategy(|chain| {
                chain
                    .next(|tcp, core| async move {
                        
                        if core.test("socks5") {
                            let _ = tcp.begin().await;
                            match tcp.clone().authenticate({
                                match core.config.socks_passwd.as_ref() {
                                    None => None,
                                    Some(password) => Some(Arc::new(PasswordAuth(password.clone()))),
                                }
                            }).await {
                                Ok(Socks::Udp(forward)) => {
                                    log::info!("[udp_forward] {}", tcp.peer_addr().unwrap());
    
                                    core.udp_forward(|listen, mut udp| {
                                        forward.resolve(listen.local_addr(), || async move {
                                            let mut stream = listen.accept().await?;
                                            let packet = stream.recv_udp().await?;
                                            let addr = match packet.addr() {
                                                fuso_socks::Addr::Socket(addr) => {
                                                    fuso_core::packet::Addr::Socket(addr)
                                                }
                                                fuso_socks::Addr::Domain(domain, port) => {
                                                    fuso_core::packet::Addr::Domain(domain, port)
                                                }
                                            };
    
                                            let data = udp.call(addr, packet.data()).await?;
    
                                            stream.send_udp(([0, 0, 0, 0], 0).into(), &data).await?;
    
                                            Ok(())
                                        })
                                    })
                                    .await?;
    
                                    Ok(State::Release)
                                }
                                Ok(Socks::Tcp(tcp, addr)) => Ok(State::Accept(Action::Forward(0, {
                                    log::debug!("[socks] {}", addr);
                                    let _ = tcp.release().await;
                                    match addr {
                                        fuso_socks::Addr::Socket(addr) => {
                                            fuso_core::packet::Addr::Socket(addr)
                                        }
                                        fuso_socks::Addr::Domain(domain, port) => {
                                            fuso_core::packet::Addr::Domain(domain, port)
                                        }
                                    }
                                }))),
                                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                                    let _ = tcp.back().await;
                                    Ok(State::Next)
                                }
                                Err(e) => Err(e.into()),
                            }
                        }else{
                            Ok(State::Next)
                        }
                    })
                    .next(|_, _| async move {
                        Ok(State::Accept(Action::Forward(
                            0,
                            Addr::Socket(([0, 0, 0, 0], 0).into()),
                        )))
                    })
            })
            .build()
            .map_ok(|fuso| {
                let ex = Executor::new();
                smol::block_on(ex.run(fuso.for_each(move |stream| async move {
                    
                    let xor = Xor::new(xor_secret);
                    let (from, to, _) = stream.split();
                    
                    let to = to.cipher(xor.clone()).await;

                    from.forward(to).detach();
                })));
            })
            .map_err(|e| async move {
                log::error!("{}", e);
            })
            .await
            .ok();
    };

    smol::block_on(future);
}
