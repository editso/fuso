use fuso::{
    protocol::AsyncRecvPacket, service::Transfer, AsyncRead, AsyncWrite, FusoStream, Stream,
};

async fn t() -> impl Transfer<Output = FusoStream> {
    tokio::net::TcpStream::connect("").await.unwrap()
}

#[cfg(feature = "fuso-rt-tokio")]
#[tokio::main]
async fn main() -> fuso::Result<()> {
    use std::{sync::Arc, time::Duration};

    use fuso::{
        ext::{AsyncReadExt, AsyncWriteExt},
        guard::{Fallback, Timer},
        listener::ext::AccepterExt,
        middleware::Handshake,
        protocol::AsyncRecvPacket,
        select::Select,
        server::penetrate::{PenetrateFactory, PeerFactory},
        service::ServerFactory,
        FusoStream, Stream, TokioExecutor,
    };

    use tokio::net::TcpStream;

    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .default_format()
        .format_module_path(false)
        .init();

    fuso::builder_server_with_tokio()
        .with_handshake(Handshake)
        .with_penetrate()
        .max_wait_time(Duration::from_secs(5))
        .heartbeat_timeout(Duration::from_secs(20))
        .build(PeerFactory)
        .bind(([0, 0, 0, 0], 8888))
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
    use std::time::Duration;

    use fuso::{
        ext::{AsyncReadExt, AsyncWriteExt},
        guard::{Fallback, Timer},
        net::tun::Tun,
        tun::TunProvider,
        Fuso, SmolExecutor, Stream, TcpListener,
    };
    use smol::net::TcpStream;

    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    smol::block_on(async move {
        let tcp = TcpStream::connect("xxtests.xyz:80").await?;

        // let fallback = Fallback::new(tcp);
        let tcp = Timer::with_read_write(tcp, Duration::from_secs(10));

        let mut f: Box<dyn Stream> = Box::new(tcp);
        f.write(b"GET / HTTP/1.1\r\n\r\n").await;
        let mut buf = Vec::new();
        buf.resize(10, 0);
        f.read(&mut buf).await;

        println!("{:?}", buf);

        Ok(())
    })
}

#[cfg(feature = "fuso-rt-custom")]
fn main() {}
