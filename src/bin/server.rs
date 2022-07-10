#[cfg(feature = "fuso-rt-tokio")]
#[tokio::main]
async fn main() -> fuso::Result<()> {
    use std::time::Duration;

    use fuso::{Handshake, Socket, UdpForwardFactory};

    env_logger::builder()
        .filter_module("fuso", log::LevelFilter::Debug)
        .default_format()
        .format_module_path(false)
        .init();

    fuso::builder_server_with_tokio()
        .with_handshake(Handshake)
        .with_penetrate()
        .read_timeout(None)
        .max_wait_time(Duration::from_secs(5))
        .heartbeat_timeout(Duration::from_secs(10))
        .with_adapter_mode()
        .with_normal_unpacker()
        .with_socks_unpacker()
        .with_udp_forward(UdpForwardFactory)
        .build()
        .bind(Socket::Tcp(([0, 0, 0, 0], 8888).into()))
        .run()
        .await
        .expect("server start failed");

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
    use fuso::{Handshake, Socket};

    env_logger::builder()
        .filter_module("fuso", log::LevelFilter::Trace)
        .default_format()
        .format_module_path(false)
        .init();

    smol::block_on(async move {
        fuso::builder_server_with_smol()
            .with_handshake(Handshake)
            .with_penetrate()
            .with_adapter_mode()
            .use_normal()
            .use_socks()
            .build()
            .bind(Socket::Tcp(([0, 0, 0, 0], 8888).into()))
            .run()
            .await
    })
}
