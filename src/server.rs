use std::process::exit;

use clap::{App, Arg};
use fuso::parse_addr;
use fuso_core::{server::Fuso, Forward, FusoListener};

fn main() {
    let app = App::new("fuso")
        .version("1.0")
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
        match Fuso::bind((server_vis_addr, server_bind_addr)).await {
            Ok(mut fuso) => loop {
                match fuso.accept().await {
                    Ok((from, to)) => {
                        let _ = to.spwan_forward(from);
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
