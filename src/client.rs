use std::{net::SocketAddr, process::exit, time::Duration};

use clap::{App, Arg};
use fuso::parse_addr;
use fuso_core::{client::Fuso, Forward, FusoListener};
use fuso_socks::{DefauleDnsResolve, PasswordAuth, Socks};
use smol::{
    io::AsyncWriteExt,
    net::{TcpStream, UdpSocket},
};

#[derive(Debug, Clone, Copy)]
enum Proxy {
    Port(SocketAddr),
    Socks5,
}

#[inline]
async fn poll_stream(mut fuso: Fuso, proxy: Proxy) -> fuso_core::Result<()> {
    loop {
        let reactor = fuso.accept().await?;
        let stream = reactor.join().await?;

        match proxy {
            Proxy::Socks5 => {
                let future = async move {
                    let socks = Socks::parse(
                        stream,
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

                    if let Ok(socks) = socks {
                        match socks {
                            Socks::Tcp(mut from, addr) => {
                                let to = TcpStream::connect(addr).await;

                                if to.is_ok() {
                                    let to = to.unwrap();
                                    log::info!(
                                        "[fuc] Proxy to {} -> {}",
                                        from.peer_addr().unwrap(),
                                        to.local_addr().unwrap()
                                    );

                                    if let Err(e) = to.forward(from).await {
                                        log::warn!("[fuc] Forwarding failed {}", e);
                                    };
                                } else {
                                    log::warn!(
                                        "[fuc] Proxy failure({}) {} -> {}",
                                        to.unwrap_err(),
                                        from.peer_addr().unwrap(),
                                        addr
                                    );

                                    let _ = from.close().await;
                                }
                            }
                            Socks::Udp(mut tcp, _) => {
                                // 伪代码 ...
                                log::warn!("[fuc] Not support udp forwarding");
                                let _ = tcp.close().await;
                            }
                        }
                    }
                };

                smol::spawn(future).detach();
            }
            Proxy::Port(addr) => {
                let future = async move {
                    let tcp = TcpStream::connect(addr).await;
                    if tcp.is_err() {
                        log::warn!("[fuc] Connection failed {} {}", tcp.unwrap_err(), addr);
                    } else {
                        log::info!(
                            "[fuc] Forward to {} -> {}",
                            addr,
                            stream.peer_addr().unwrap()
                        );

                        if let Err(e) = stream.forward(tcp.unwrap()).await {
                            log::warn!("[fuc] Forwarding failed {}", e);
                        }
                    }
                };

                smol::spawn(future).detach();
            }
        };
    }
}

fn main() {
    let app = App::new("fuso")
        .version("1.0")
        .author("https://github.com/editso/fuso")
        .arg(Arg::new("server-host").default_value("127.0.0.1"))
        .arg(Arg::new("server-port").default_value("9003"))
        .arg(
            Arg::new("forward-host")
                .short('h')
                .long("host")
                .default_value("127.0.0.1"),
        )
        .arg(
            Arg::new("forward-port")
                .short('p')
                .long("port")
                .default_value("80"),
        )
        .arg(
            Arg::new("forward-type")
                .short('t')
                .long("type")
                .possible_values(["socks", "forward"])
                .default_value("forward"),
        )
        .arg(
            Arg::new("log")
                .short('l')
                .possible_values(["debug", "info", "trace", "error"])
                .default_value("info"),
        );

    let matches = app.get_matches();

    let server_addr = parse_addr(
        matches.value_of("server-host").unwrap(),
        matches.value_of("server-port").unwrap(),
    );

    let forward_addr = parse_addr(
        matches.value_of("forward-host").unwrap(),
        matches.value_of("forward-port").unwrap(),
    );

    let forward_type = matches.value_of("forward-type").unwrap();

    if server_addr.is_err() {
        println!("Parameter error: {}", server_addr.unwrap_err());
        exit(1);
    }

    if forward_addr.is_err() {
        println!("Parameter error: {}", forward_addr.unwrap_err());
        exit(1);
    }

    let forward_addr = forward_addr.unwrap();
    let server_addr = server_addr.unwrap();

    let proxy = match forward_type {
        "socks" => Proxy::Socks5,
        _ => Proxy::Port(forward_addr),
    };

    env_logger::builder()
        .filter_level(match matches.value_of("log").unwrap() {
            "debug" => log::LevelFilter::Debug,
            "info" => log::LevelFilter::Info,
            "warn" => log::LevelFilter::Warn,
            "error" => log::LevelFilter::Error,
            _ => log::LevelFilter::Info,
        })
        .filter_module("fuso_socks", log::LevelFilter::Info)
        .init();

    log::info!(
        "[fuc] server_addr={}, forward_type={}, forward_addr={}",
        server_addr,
        forward_type,
        forward_addr
    );

    smol::block_on(async move {
        loop {
            match Fuso::bind(server_addr).await {
                Ok(fuso) => {
                    log::info!("[fuc] connection succeeded");
                    let _ = poll_stream(fuso, proxy.clone()).await;
                }
                Err(e) => {
                    log::warn!(
                        "[fuc] Unable to connect to the server {} {}",
                        e,
                        server_addr
                    );
                }
            }

            smol::Timer::after(Duration::from_secs(2)).await;

            log::debug!("[fuc] Try to reconnect");
        }
    });
}
