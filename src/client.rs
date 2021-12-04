use std::{net::SocketAddr, process::exit, time::Duration};

use clap::{App, Arg};
use fuso::parse_addr;
use fuso_core::{
    ciphe::{Cipher, Security, Xor},
    client::Fuso,
    Forward, FusoListener,
};
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
async fn poll_stream<C>(mut fuso: Fuso, proxy: Proxy, ciphe: C) -> fuso_core::Result<()>
where
    C: Clone + Cipher + Unpin + Send + Sync + 'static,
{
    loop {
        let reactor = fuso.accept().await?;
        let stream = reactor.join().await?;
        let from_addr = stream.peer_addr().unwrap();

        let stream = stream.ciphe(ciphe.clone()).await;

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
                                        from_addr,
                                        to.local_addr().unwrap()
                                    );

                                    if let Err(e) = to.forward(from).await {
                                        log::warn!("[fuc] Forwarding failed {}", e);
                                    };
                                } else {
                                    log::warn!(
                                        "[fuc] Proxy failure({}) {} -> {}",
                                        to.unwrap_err(),
                                        from_addr,
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
                        log::info!("[fuc] Forward to {} -> {}", addr, from_addr);

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
        .version("v1.0.3")
        .author("https://github.com/editso/fuso")
        .arg(
            Arg::new("server-host")
                .default_value("127.0.0.1")
                .about("服务端监听的地址"),
        )
        .arg(
            Arg::new("server-port")
                .default_value("9003")
                .about("服务端监听的端口"),
        )
        .arg(
            Arg::new("forward-host")
                .short('h')
                .long("host")
                .default_value("127.0.0.1")
                .display_order(1)
                .about("转发地址, (如果开启了socks代理该参数将无效)"),
        )
        .arg(
            Arg::new("forward-port")
                .short('p')
                .long("port")
                .default_value("80")
                .display_order(2)
                .about("转发的端口 (如果开启了socks代理该参数将无效)"),
        )
        .arg(
            Arg::new("forward-type")
                .short('t')
                .long("type")
                .possible_values(["socks", "forward"])
                .default_value("forward")
                .display_order(3)
                .about("转发类型"),
        )
        .arg(
            Arg::new("service-bind-port")
                .short('b')
                .long("bind")
                .validator(|port| {
                    port.parse::<u16>()
                        .map_or(Err(format!("Invalid port {}", port)), |_| Ok(()))
                })
                .display_order(5)
                .takes_value(true)
                .about("真实映射成功后访问的端口号, 不指定将自动分配"),
        )
        .arg(
            Arg::new("xor-secret")
                .long("xor")
                .short('x')
                .default_value("27")
                .validator(|num| {
                    num.parse::<u8>()
                        .map_or(Err(String::from("Invalid number 0-255")), |_| Ok(()))
                })
                .display_order(4)
                .about(
                    "传输时使用异或加密的Key (Use exclusive OR encrypted key during transmission)",
                ),
        )
        .arg(
            Arg::new("bridge-bind-host")
                .long("bridge-host")
                .takes_value(true)
                .display_order(5)
                .about("桥接服务监听的地址"),
        )
        .arg(
            Arg::new("bridge-bind-port")
                .long("bridge-port")
                .takes_value(true)
                .validator(|port| {
                    port.parse::<u16>()
                        .map_or(Err(format!("Invalid port {}", port)), |_| Ok(()))
                })
                .display_order(6)
                .about("桥接服务监听的端口"),
        )
        .arg(
            Arg::new("name")
                .long("name")
                .short('n')
                .takes_value(true)
                .display_order(7)
                .about("自定义当前映射服务的名称"),
        )
        .arg(
            Arg::new("log")
                .short('l')
                .possible_values(["debug", "info", "trace", "error"])
                .default_value("info")
                .about("日志级别"),
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

    let name = matches
        .value_of("name")
        .map_or_else(|| None, |name| Some(name.to_string()));

    let service_bind_port: u16 = matches
        .value_of("service-bind-port")
        .unwrap_or("0")
        .parse()
        .unwrap();

    let forward_type = matches.value_of("forward-type").unwrap();

    if server_addr.is_err() {
        println!("Server address error: {}", server_addr.unwrap_err());
        exit(1);
    }

    if forward_addr.is_err() {
        println!("Parameter error: {}", forward_addr.unwrap_err());
        exit(1);
    }

    let forward_addr = forward_addr.unwrap();
    let server_addr = server_addr.unwrap();

    let xor_num: u8 = matches.value_of("xor-secret").unwrap().parse().unwrap();

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

    let bridge_addr = {
        let bridge_bind_host = matches.value_of("bridge-bind-host").unwrap_or("0.0.0.0");
        let bridge_bind_port = matches.value_of("bridge-bind-port");

        if bridge_bind_port.is_none() {
            None
        } else {
            parse_addr(bridge_bind_host, bridge_bind_port.unwrap()).map_or_else(
                |s| {
                    log::warn!("Incorrect bridge parameters: {}", s);
                    None
                },
                |addr| Some(addr),
            )
        }
    };

    log::info!(
        "\nserver_addr={}\nforward_type={}\n{}\nxor_num={}\n{}\n",
        server_addr,
        forward_type,
        {
            match proxy {
                Proxy::Port(_) => format!("forward_addr={}", forward_addr),
                Proxy::Socks5 => "--".to_string(),
            }
        },
        xor_num,
        {
            if bridge_addr.is_some() {
                format!("bridge_addr={}", bridge_addr.clone().unwrap())
            } else {
                "--".to_string()
            }
        }
    );

    smol::block_on(async move {
        loop {
            match Fuso::bind(fuso_core::client::Config {
                name: name.clone(),
                server_addr,
                server_bind_port: service_bind_port,
                bridge_addr: bridge_addr,
            })
            .await
            {
                Ok(fuso) => {
                    log::info!("[fuc] connection succeeded");
                    let _ = poll_stream(fuso, proxy.clone(), Xor::new(xor_num)).await;
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
