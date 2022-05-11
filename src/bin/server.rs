#[cfg(feature = "fuso-rt-tokio")]
#[tokio::main]
async fn main() -> fuso::Result<()> {
    use std::sync::Arc;

    use fuso::{
        net::{socks::Socks5, tun::Tun},
        server::{self, builder::FusoServer},
        Executor, TokioExecutor,
    };
    use tokio::net::TcpListener;

    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    FusoServer::new()
        .service(Tun::new())
        // .service(Proxy::new())
        .serve(TcpListener::bind("0.0.0.0:9999").await?)
        .await?;

    Ok(())
}

#[cfg(feature = "fuso-web")]
#[tokio::main]
async fn main() {}

#[cfg(feature = "fuso-api")]
#[tokio::main]
async fn main() {}

#[cfg(feature = "fuso-rt-smol")]
fn main() -> fuso::Result<()> {
    use fuso::{
        net::tun::Tun, server::builder::FusoServer, service::Service, tun::TunProvider, TcpListener,
    };

    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    smol::block_on(async move {
        // let tun = Tun::new();
        FusoServer::new()
            .encryption()
            .service(Tun::new(TunProvider))
            .serve(TcpListener::bind("0.0.0.0:9999").await?)
            .await
    })
}

#[cfg(feature = "fuso-rt-custom")]
fn main() {}
