use clap::Parser;

use crate::{config::client::Config, error};

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct App {
    #[arg(short, long)]
    config: Option<String>,
}

pub fn parse() -> error::Result<Config> {
    let mut app = App::parse();

    app.config.replace(format!("config/client.toml"));

    let cfg = match app.config {
        None => Config::default(),
        Some(cfg) => toml::from_str(&{ std::fs::read_to_string(cfg)? })?,
    };

    Ok(cfg)
}
