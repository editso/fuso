use std::process::exit;

use clap::{App, Arg};
use fuso::parse_addr;
use fuso_core::{
    ciphe::{Security, Xor},
    core::{Config, Fuso},
    dispatch::State,
    packet::{Action, Addr},
    Forward, FusoListener, FusoPacket, Spwan,
};

use fuso_socks::{Socks, Socks5Ex};

fn main() {
    let app = App::new("fuso")
        .version("v1.0.3")
        .author("https://github.com/editso/fuso")
        .arg(
            Arg::new("server-bind-host")
                .default_value("0.0.0.0")
                .long("host")
                .short('h')
                .display_order(1)
                .about("监听地址"),
        )
        .arg(
            Arg::new("server-bind-port")
                .default_value("9003")
                .long("port")
                .short('p')
                .display_order(2)
                .about("监听端口"),
        )
        .arg(
            Arg::new("xor-secret")
                .long("xor")
                .short('x')
                .default_value("27")
                .display_order(3)
                .validator(|num| {
                    num.parse::<u8>()
                        .map_or(Err(String::from("Invalid number 0-255")), |_| Ok(()))
                })
                .about("传输时使用异或加密的Key"),
        )
        .arg(
            Arg::new("log")
                .short('l')
                .display_order(4)
                .possible_values(["debug", "info", "trace", "error"])
                .default_value("info")
                .about("日志级别"),
        );

    let matches = app.get_matches();

    let server_bind_addr = parse_addr(
        matches.value_of("server-bind-host").unwrap(),
        matches.value_of("server-bind-port").unwrap(),
    );

    if server_bind_addr.is_err() {
        println!("Parameter error: {}", server_bind_addr.unwrap_err());
        exit(1);
    }

    let server_bind_addr = server_bind_addr.unwrap();

    let xor_num: u8 = matches.value_of("xor-secret").unwrap().parse().unwrap();

    env_logger::builder()
        .filter_level(match matches.value_of("log").unwrap() {
            "debug" => log::LevelFilter::Debug,
            "info" => log::LevelFilter::Info,
            "warn" => log::LevelFilter::Warn,
            "error" => log::LevelFilter::Error,
            _ => log::LevelFilter::Info,
        })
        .init();

    smol::block_on(async move {
        let fuso = Fuso::builder()
            .with_config(Config {
                debug: false,
                bind_addr: server_bind_addr,
            })
            .with_chain(|chain| {
                chain.next(|mut tcp, cx| async move {
                    let action: Action = tcp.recv().await?.try_into()?;
                    match action {
                        Action::Bind(name, addr) => match cx.spwan(tcp.clone(), addr, name).await {
                            Ok(conv) => {
                                log::debug!(
                                    "[fuso] accept conv={}, addr={}",
                                    conv,
                                    tcp.peer_addr().unwrap(),
                                );
                                Ok(State::Accept(()))
                            }
                            Err(e) => {
                                log::warn!("[fuso] Failed to open the mapping {}", e.to_string());
                                let _ = tcp.send(Action::Err(e.to_string()).into()).await?;
                                Ok(State::Accept(()))
                            }
                        },
                        Action::Connect(conv) => {
                            cx.route(conv, tcp.into()).await?;
                            Ok(State::Accept(()))
                        }
                        _ => Ok(State::Next),
                    }
                })
            })
            .chain_strategy(|chain| {
                chain
                    .next(|tcp, _| async move {
                        let _ = tcp.begin().await;
                        match tcp.clone().authenticate(None).await {
                            Ok(Socks::Udp(udp)) => {
                                let _ = udp.reject().await;
                                Ok(State::Release)
                            }
                            Ok(Socks::Tcp(_, addr)) => Ok(State::Accept(Action::Forward({
                                log::info!("[socks] {}", addr);

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
                    })
                    .next(|_, _| async move {
                        Ok(State::Accept(Action::Forward(Addr::Socket(
                            ([0, 0, 0, 0], 0).into(),
                        ))))
                    })
            })
            .build()
            .await;

        match fuso {
            Ok(mut fuso) => loop {
                match fuso.accept().await {
                    Ok(stream) => {
                        let (from, to) = stream.split();

                        let to = to.ciphe(Xor::new(xor_num)).await;

                        to.forward(from).detach();
                    }
                    Err(e) => {
                        log::warn!("[fuso] Server error {}", e.to_string());
                        break;
                    }
                }
            },
            Err(_) => {
                log::error!(
                    "[fus] Invalid address or already used . bind={}",
                    server_bind_addr
                )
            }
        }
    });
}
