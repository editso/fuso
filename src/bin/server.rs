use std::{net::SocketAddr, str::FromStr};

use fuso::{
    config::{server::Config, Stateful},
    core::{
        accepter::{AccepterExt, MultiAccepter, StreamAccepter, TaggedAccepter},
        future::Select,
        handshake::Handshaker,
        io::{AsyncReadExt, AsyncWriteExt, StreamExt},
        net::TcpListener,
        processor::{IProcessor, Processor, StreamProcessor},
        protocol::{AsyncPacketRead, AsyncPacketSend},
        split::SplitStream,
        stream::{fallback::Fallback, handshake::Handshake},
    },
    error,
    server::{
        manager::{Manager, MultiServiceManager},
        port_forward::{MuxAccepter, PortForwarder},
    },
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

    enter_fuso_serve(Stateful::new(conf)).await?;
    // axum::serve(tcp_listener, make_service)

    loop {

        // mgr.manage(|ctrl|async move{

        // });

        // mgr.service(|mgr| async move {
        //     enter_port_forward_service().await
        // });

        // let a = processor.process(a).await;

        // let a = PortForward::new(a, ShareAccepter::new(
        //     accepter,
        //     // Rc4Recognizer(prefix, a)
        // ));

        // let (from, to) = a.accept().await?;

        // let n = reader.read(&mut buf).await?;

        // let (k, s) = a.into_inner();

        // assert_eq!(k, Some(buf[..n].to_vec()));

        // println!("{:?}", String::from_utf8_lossy(&buf[..n]));

        // println!("{:?} => {}", tag, addr);
    }

    Ok(())
}

// pub async fn enter_port_forward_service(mgr){
//     let (tag, (addr, transport)) = accepter.accept().await?;

//     let mut forwarder = PortForwarder::new(
//         transport,
//         {
//             ShareAccepter::new(1110, rand::random(), {
//                 StreamAccepter::new({
//                     fuso::core::net::TcpListener::bind(
//                         SocketAddr::from_str("0.0.0.0:9999").unwrap(),
//                     )
//                     .await?
//                 })
//             })
//         },
//         (),
//         None,
//     );

//     let mgr = 0;

//     let (c1, c2) = forwarder.accept().await?;

//     mgr.spawn(PortForward(), async move{
//         c1.copy(c2);
//     });

//     unimplemented!()
// }

async fn enter_fuso_serve(conf: Stateful<Config>) -> error::Result<()> {
    let mut accepter = MultiAccepter::new();

    for listen in &conf.listens {
        match listen {
            fuso::config::server::Listen::Kcp(kcp) => {
                accepter.add(TaggedAccepter::new(
                    Tagged::Kcp,
                    StreamAccepter::new({
                        fuso::core::net::KcpListener::bind(Default::default(), kcp.as_socket_addr())
                            .await?
                    }),
                ));
            }
            fuso::config::server::Listen::Tcp(tcp) => {
                accepter.add(TaggedAccepter::new(
                    Tagged::Tcp,
                    StreamAccepter::new({
                        fuso::core::net::TcpListener::bind(tcp.as_socket_addr()).await?
                    }),
                ));
            }
            fuso::config::server::Listen::Proxy(proxy) => {
                accepter.add(TaggedAccepter::new(
                    Tagged::Tcp,
                    StreamAccepter::new({
                        fuso::core::net::TcpListener::bind(proxy.as_socket_addr()).await?
                    }),
                ));
            }
            fuso::config::server::Listen::Tunnel(forward) => {
                accepter.add(TaggedAccepter::new(
                    Tagged::Tcp,
                    StreamAccepter::new({
                        fuso::core::net::TcpListener::bind(forward.as_socket_addr()).await?
                    }),
                ));
            }
        }
    }

    loop {
        let (addr, (_, transport)) = accepter.accept().await?;


        let mut forwarder = PortForwarder::new(
            transport,
            {
                MuxAccepter::new(1110, rand::random(), {
                    StreamAccepter::new({
                        fuso::core::net::TcpListener::bind(
                            SocketAddr::from_str("0.0.0.0:9999").unwrap(),
                        )
                        .await?
                    })
                })
            },
            (),
            None,
        );

        let mgr = 0;

        let (c1, c2) = forwarder.accept().await?;
    }

    let s = fuso::server::Serve {};

    Ok(())
}
