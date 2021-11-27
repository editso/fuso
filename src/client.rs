use std::{process::exit, time::Duration};

use clap::{App, Arg};
use fuso::parse_addr;
use fuso_core::{client::Fuso, FusoListener};
use smol::net::TcpStream;

fn main() {
    let app = App::new("fuso")
        .version("1.0")
        .author("editso ")
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
        .arg(Arg::new("log").short('l').default_value("debug"));

    let matches = app.get_matches();

    let server_addr = parse_addr(
        matches.value_of("server-host").unwrap(),
        matches.value_of("server-port").unwrap(),
    );

    let forward_addr = parse_addr(
        matches.value_of("forward-host").unwrap(),
        matches.value_of("forward-port").unwrap(),
    );

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
        loop {
            match Fuso::bind(server_addr).await {
                Ok(mut fuso) => {
                    log::debug!("[fuc] Successfully connected {}", server_addr);
                    loop {
                        match fuso.accept().await {
                            Ok(reac) => {
                                smol::spawn(async move {
                                    match TcpStream::connect(forward_addr).await {
                                        Ok(fus) => match reac.join().await {
                                            Ok(stream) => {
                                                log::debug!(
                                                    "[fuc] {} -> {}",
                                                    fus.local_addr().unwrap(),
                                                    stream.local_addr().unwrap()
                                                );

                                                if let Err(e) =
                                                    fuso_core::forward(fus, stream).await
                                                {
                                                    log::warn!("[fuc] {}", e);
                                                };
                                            }
                                            Err(e) => {
                                                log::warn!("[fuc] {}", e)
                                            }
                                        },
                                        Err(_) => {
                                            log::warn!("[fuc] ");
                                        }
                                    };
                                })
                                .detach();
                            }
                            Err(_) => {
                                break;
                            }
                        }
                    }
                }
                Err(_) => {
                    log::warn!("[fuc] Unable to connect to the server {}", server_addr)
                }
            }

            smol::Timer::after(Duration::from_secs(2)).await;

            log::debug!("[fuc] Try to reconnect");
        }
    });
}
