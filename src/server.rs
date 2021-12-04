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
use fuso_socks::{DefauleDnsResolve, PasswordAuth, Socks};
use smol::net::UdpSocket;

fn main() {
    let app = App::new("fuso")
        .version("v1.0.3")
        .author("https://github.com/editso/fuso")
        .arg(Arg::new("server-bind-host").default_value("0.0.0.0"))
        .arg(Arg::new("server-bind-port").default_value("9003"))
        .arg(
            Arg::new("server-visit-host")
                .long("host")
                .short('h')
                .default_value("0.0.0.0"),
        )
        .arg(
            Arg::new("server-visit-port")
                .long("port")
                .short('p')
                .default_value("0"),
        )
        .arg(
            Arg::new("xor-secret")
                .long("xor")
                .short('x')
                .default_value("27")
                .validator(|num| {
                    num.parse::<u8>()
                        .map_or(Err(String::from("Invalid number 0-255")), |_| Ok(()))
                }),
        )
        .arg(
            Arg::new("log")
                .short('l')
                .possible_values(["debug", "info", "trace", "error"])
                .default_value("info"),
        );

    let matches = app.get_matches();

    let server_bind_addr = parse_addr(
        matches.value_of("server-bind-host").unwrap(),
        matches.value_of("server-bind-port").unwrap(),
    );

    let server_vis_addr = parse_addr(
        matches.value_of("server-visit-host").unwrap(),
        matches.value_of("server-visit-port").unwrap(),
    );

    if server_bind_addr.is_err() {
        println!("Parameter error: {}", server_bind_addr.unwrap_err());
        exit(1);
    }

    if server_vis_addr.is_err() {
        println!("Parameter error: {}", server_vis_addr.unwrap_err());
        exit(1);
    }

    let server_vis_addr = server_vis_addr.unwrap();
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
                        Action::Bind(addr) => match cx.spwan(tcp.clone().into(), addr).await {
                            Ok(conv) => {
                                log::debug!("[fuso] accept {}", conv);
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
                    .next(|_, _| async move {
                        Ok(State::Accept(Action::Forward(Addr::Socket(
                            "127.0.0.1:80".parse().unwrap(),
                        ))))
                    })
                    .next(|tcp, _| async move {
                        // 下面是测试性的, 永远都不会执行到这里
                        let io = tcp.clone();
                        let socks = Socks::parse(
                            io,
                            |_, _| async move {
                                let udp = UdpSocket::bind("0.0.0.0:0").await;
                                if udp.is_err() {
                                    Err(udp.unwrap_err())
                                } else {
                                    // ..... Invalid this is
                                    let udp = udp.unwrap();
                                    Ok((udp.clone(), udp.local_addr().unwrap()))
                                }
                            },
                            &PasswordAuth::default(),
                            &DefauleDnsResolve::default(),
                        )
                        .await;

                        if socks.is_err() {
                            let _ = tcp.back().await;
                            log::debug!("Not a valid socks package");
                            Ok(State::Next)
                        } else {
                            match socks.unwrap() {
                                Socks::Udp(_, _) => Err("Does not support udp forwarding".into()),
                                Socks::Tcp(_, addr) => {
                                    Ok(State::Accept(Action::Forward(Addr::Socket(addr))))
                                }
                            }
                        }
                    })
            })
            .build()
            .await;

        match fuso {
            Ok(mut fuso) => loop {
                match fuso.accept().await {
                    Ok(stream) => {
                        let to = stream.to.ciphe(Xor::new(xor_num)).await;

                        to.forward(stream.from).detach();
                    }
                    Err(e) => {
                        log::warn!("[fuso] Server error {}", e.to_string());
                        break;
                    }
                }
            },
            Err(_) => {
                log::error!(
                    "[fus] Invalid address or already used . bind={}, visit={}",
                    server_bind_addr,
                    server_vis_addr
                )
            }
        }
    });
}
