use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    str::FromStr,
    time::Duration,
};

use fuso::{
    config::{client::WithForwardService, server::Config, Expose, Stateful},
    core::{
        accepter::{AccepterExt, MultiAccepter, StreamAccepter, TaggedAccepter},
        future::{Poller, Select},
        handshake::Handshaker,
        io::{AsyncReadExt, AsyncWriteExt},
        net::{KcpListener, TcpListener, UdpSendTo, UdpSocket},
        processor::{IProcessor, PreprocessorSelector, Processor, StreamProcessor},
        protocol::{AsyncPacketRead, AsyncPacketSend},
        rpc::Decoder,
        split::SplitStream,
        stream::{
            fallback::Fallback,
            handshake::{ClientConfig, ForwardConfig, Handshake, MuxConfig},
            UseCompress, UseCrypto,
        },
        transfer::TransmitterExt,
        AbstractStream, Stream,
    },
    error,
    runtime::tokio::{TokioRuntime, TokioUdpSocket, UdpWithTokioRuntime},
    server::{
        port_forward::{ForwardAccepter, MuxAccepter, PortForwarder, Socks5Preprocessor},
        udp_forward::UdpForwarderImpl,
    },
};
use kcp_rust::KcpConfig;

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
        .filter_module("fuso_socks", log::LevelFilter::Error)
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
        let (_, (addr, transport)) = accepter.accept().await?;

        log::debug!("connection from {addr}");
        let conf = conf.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(conf, transport).await {
                log::error!("transport closed {e:?}");
            }
        });
    }
}

pub async fn handle_connection(
    conf: Stateful<Config>,
    stream: AbstractStream<'static>,
) -> error::Result<()> {
    let crypto = &conf.crypto;
    let compress = &conf.compress;

    let mut stream = stream
        .use_crypto(crypto.iter())
        .use_compress(compress.iter())
        .server_handshake::<TokioRuntime>(
            &conf.authentication,
            Duration::from_secs(conf.authentication_timeout as _),
        )
        .await?;

    match stream.read_config().await? {
        ClientConfig::Forward(forward) => enter_forwarder(forward, stream).await?,
    }

    Ok(())
}

pub async fn enter_forwarder<S>(conf: ForwardConfig, mut transport: S) -> error::Result<()>
where
    S: Stream + Send + Unpin + 'static,
{
    log::debug!("{:#?}", conf);

    let mut accepter = MultiAccepter::new();

    if let Some(ref channels) = conf.channel {
        for expose in conf.exposes.iter() {
            match expose {
                Expose::Kcp(ip, port) => {
                    accepter.add(ForwardAccepter::new_visitor(
                        KcpListener::bind(Default::default(), ip.to_addr(*port)).await?,
                    ));
                }
                Expose::Tcp(ip, port) => accepter.add(ForwardAccepter::new_visitor(
                    TcpListener::bind(ip.to_addr(*port)).await?,
                )),
            }
        }

        for expose in channels.iter() {
            match expose {
                Expose::Tcp(ip, port) => {
                    accepter.add(ForwardAccepter::new_mapping(
                        TcpListener::bind(ip.to_addr(*port)).await?,
                    ));
                }
                Expose::Kcp(ip, port) => {
                    accepter.add(ForwardAccepter::new_mapping({
                        KcpListener::bind(Default::default(), ip.to_addr(*port)).await?
                    }));
                }
            }
        }
    } else {
        accepter.add({
            log::debug!("using mux accepter");

            let mux: MuxConfig = transport.recv_packet().await?.decode()?;

            let mut accepter = MultiAccepter::new();

            for expose in conf.exposes.iter() {
                match expose {
                    Expose::Tcp(ip, port) => {
                        accepter.add(StreamAccepter::new(
                            TcpListener::bind(ip.to_addr(*port)).await?,
                        ));
                    }
                    Expose::Kcp(ip, port) => {
                        accepter.add(StreamAccepter::new({
                            KcpListener::bind(Default::default(), ip.to_addr(*port)).await?
                        }));
                    }
                }
            }

            MuxAccepter::new(mux, accepter)
        });
    }

    let mut poller = Poller::new();
    let mut previs = PreprocessorSelector::new();

    if let Some(ref socks5) = conf.prepares.socks5 {
        previs.add({
            if let Some(ref udp) = socks5.udp {
                let (addr, udp) =
                    UdpSocket::bind::<UdpWithTokioRuntime, _>((udp.bind, udp.port)).await?;
                let (task, forwarder) = UdpForwarderImpl::new(addr, udp);
                poller.add(task);
                Socks5Preprocessor::new(forwarder)
            } else {
                Socks5Preprocessor::new(())
            }
        })
    }

    let mut forwarder = PortForwarder::new(transport, accepter, previs, None);

    log::debug!("port forwarder started .");

    poller.add(async move {
        loop {
            let (c1, c2) = forwarder.accept().await?;

            log::debug!("start forward {} -> {}", c1.addr(), c2.addr());

            tokio::spawn(async move {
                let _ = c1.transfer(c2).await;
            });
        }
    });

    poller.await
}
