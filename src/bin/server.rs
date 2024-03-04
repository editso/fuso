use std::net::SocketAddr;

use fuso::{
    config::server::Config,
    core::{
        accepter::{AccepterExt, MultiAccepter, StreamAccepter, TaggedAccepter},
        handshake::Handshake,
        io::AsyncReadExt,
        rpc::AsyncCaller,
        split::SplitStream,
        stream::fallback::Fallback,
    },
    error,
};

#[derive(Debug, Clone)]
pub enum Tagged {
    Kcp,
    Tcp,
}

#[cfg(feature = "fuso-cli")]
fn main() {
    match fuso::cli::server::parse() {
        Ok(conf) => fuso::enter_async_main(enter_fuso_main(conf)).unwrap(),
        Err(e) => {
            println!("{:?}", e)
        }
    }
}

async fn enter_fuso_main(conf: Config) -> error::Result<()> {
    //   conf.listens.len()
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    let mut accepter = MultiAccepter::new();

    for listen in conf.listens {
        match listen {
            fuso::config::server::Listen::Kcp(kcp) => {
                accepter.add(TaggedAccepter::new(
                    Tagged::Kcp,
                    StreamAccepter::new({
                        fuso::core::net::KcpListener::bind_with_tokio(
                            Default::default(),
                            kcp.as_socket_addr(),
                        )
                        .await?
                    }),
                ));
            }
            fuso::config::server::Listen::Tcp(tcp) => {
                accepter.add(TaggedAccepter::new(
                    Tagged::Tcp,
                    StreamAccepter::new({
                        fuso::core::net::TcpListener::bind_with_tokio(tcp.as_socket_addr()).await?
                    }),
                ));
            }
            fuso::config::server::Listen::Proxy(proxy) => {
                accepter.add(TaggedAccepter::new(
                    Tagged::Tcp,
                    StreamAccepter::new({
                        fuso::core::net::TcpListener::bind_with_tokio(proxy.as_socket_addr())
                            .await?
                    }),
                ));
            }
            fuso::config::server::Listen::Tunnel(forward) => {
                accepter.add(TaggedAccepter::new(
                    Tagged::Tcp,
                    StreamAccepter::new({
                        fuso::core::net::TcpListener::bind_with_tokio(forward.as_socket_addr())
                            .await?
                    }),
                ));
            }
        }
    }

    loop {
        let (tag, (addr, s)) = accepter.accept().await?;

        let mut a = s.do_handshake(&()).await?;

        

        let mut buf = [0u8; 1024];

        let (mut reader, writer) = a.split();

        let n = reader.read(&mut buf).await?;

        // let (k, s) = a.into_inner();

        // assert_eq!(k, Some(buf[..n].to_vec()));

        println!("{:?}", String::from_utf8_lossy(&buf[..n]));
        
        
        
        

        println!("{:?} => {}", tag, addr);
    }

    Ok(())
}
