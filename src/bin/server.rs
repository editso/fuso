use fuso::{AsyncRead, AsyncWrite};

#[cfg(feature = "fuso-rt-tokio")]
#[tokio::main]
async fn main() -> fuso::Result<()> {
    use std::{sync::Arc, time::Duration};

    use fuso::{
        ext::{AsyncReadExt, AsyncWriteExt},
        guard::{Fallback, Timer},
        service::ServerFactory,
        Stream, TokioExecutor,
    };

    use tokio::net::TcpStream;

    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    fuso::new_penetrate_server()
        .add_encryption(encryption)
        .build(TokioExecutor, ServerFactory::with_tokio())
        .bind(([0,0,0,0], 8888))
        .run()
        .await
        .expect("Failed to start serve");

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
