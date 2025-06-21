use std::{net::SocketAddr, sync::Arc, time::Duration};

use fuso::{
    cli,
    client::port_forward::{
        Linker, MuxTransmitterConnector, PortForwarder, Protocol, TransmitterConnector,
    },
    config::{
        client::{
            Config, FinalTarget, Host, Service, WithBridgeService, WithForwardService,
            WithProxyService,
        },
        Expose, Stateful,
    },
    core::{
        accepter::AccepterExt,
        connector::{Connector, EncryptedAndCompressedConnector, MultiConnector},
        net::{KcpConnector, TcpListener, TcpStream},
        protocol::{AsyncPacketRead, AsyncPacketSend},
        rpc::{Decoder, Encoder},
        stream::{
            fragment::Fragment,
            handshake::{Handshake, MuxConfig},
            UseCompress, UseCrypto,
        },
        transfer::TransmitterExt,
        AbstractStream,
    },
    error,
    runner::{FnRunnable, NamedRunnable, Rise, ServiceRunner},
    runtime::tokio::{TokioRuntime, UdpWithTokioRuntime},
};

macro_rules! unsupport_protocol {
    () => {
        fuso::error::Result::Err(fuso::error::FusoError::UnsupportProtocol)
    };
}

macro_rules! create_kcp_connector {
    ($conf: expr, $info: expr) => {{
        let (addr, port) = $info;
        KcpConnector::new::<UdpWithTokioRuntime, _, _>($conf, || async move {
            addr.try_connect(port, |host, port| async move {
                let udp = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
                udp.connect(format!("{host}:{port}")).await?;
                Ok(udp.into())
            })
            .await
        })
        .await
    }};
}

macro_rules! try_connect {
    (kcp => $proto: expr, $connector: expr) => {{
        match $proto {
            Protocol::Kcp => {
                let kcp = $connector.connect(()).await?;
                Ok(AbstractStream::new(kcp).into())
            }
            _ => unsupport_protocol!(),
        }
    }};
    (tcp => $proto: expr, $info: expr) => {{
        let (addr, port) = $info;
        match $proto {
            Protocol::Tcp => Ok(({
                addr.try_connect(port, |host, port| async move {
                    tokio::net::TcpStream::connect(format!("{host}:{port}"))
                        .await
                        .map_err(Into::into)
                })
                .await
            })),
            _ => unsupport_protocol!(),
        }
    }};
}

fn main() {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Debug)
        .filter_module("kcp_rust", log::LevelFilter::Trace)
        .init();

    match cli::client::parse() {
        Ok(conf) => {
            // conf.check();
            fuso::enter_async_main(enter_fuso_main(conf)).unwrap()
        }
        Err(e) => {
            log::error!("{e:?}")
        }
    }
}

fn enter_fuso_main(mut conf: Config) -> ServiceRunner<'static, TokioRuntime> {
    let mut sp = ServiceRunner::<TokioRuntime>::new();

    let services = std::mem::replace(&mut conf.services, Default::default());

    let conf = Stateful::new(conf);

    for (name, service) in services {
        let sc = conf.clone();
        match service {
            Service::Proxy(s) => {
                sp.register(
                    s.restart.clone(),
                    NamedRunnable::new(name, {
                        FnRunnable::new(move || enter_proxy_service_main(sc.clone(), s.clone()))
                    }),
                );
            }
            Service::Bridge(s) => {
                sp.register(
                    s.restart.clone(),
                    NamedRunnable::new(name, {
                        FnRunnable::new(move || enter_bridge_service_main(sc.clone(), s.clone()))
                    }),
                );
            }
            Service::Forward(s) => {
                sp.register(
                    s.restart.clone(),
                    NamedRunnable::new(name, {
                        FnRunnable::new(move || {
                            enter_port_forward_service_main(sc.clone(), s.clone())
                        })
                    }),
                );
            }
        }
    }

    sp.build()
}

async fn enter_port_forward_service_main(
    config: Stateful<Config>,
    service: WithForwardService,
) -> error::Result<Rise> {
    let server = &config.server;
    let crypto = &server.crypto;
    let compress = &server.compress;

    let mut using_mux_connector = true;

    let result = server
        .try_connect(|host, port| async move {
            log::trace!("connect to server {host}:{port}");
            TcpStream::connect(format!("{host}:{port}"))
                .await
                .map(|transport| {
                    log::debug!("connection established: {host}:{port}");
                    transport
                })
        })
        .await?
        .use_crypto(crypto.iter())
        .use_compress(compress.iter())
        .client_handshake::<TokioRuntime>(
            &server.authentication,
            Duration::from_secs(server.authentication_timeout as _),
        )
        .await;

    let mut transport = match result {
        Ok(stream) => stream,
        Err(e) => {
            log::error!("fail to handshake {e:?}");
            return Ok(Rise::Restart);
        }
    };

    transport.write_config(&service).await?;

    let connector = EncryptedAndCompressedConnector::new(service.crypto, service.compress, {
        let mut connector = MultiConnector::<Protocol, AbstractStream>::new();

        let (mux_connector, exposes) = match &service.channel {
            Some(channels) => (false, channels.iter()),
            None => (true, service.exposes.iter()),
        };

        for expose in exposes {
            match expose {
                Expose::Kcp(_, port) => {
                    let info = (server.addr.clone(), *port);
                    let kcp_connector = create_kcp_connector!(Default::default(), info)?;
                    connector.add(move |proto| {
                        let connector = kcp_connector.clone();
                        async move { try_connect!(kcp => proto, connector) }
                    })
                }
                Expose::Tcp(_, port) => {
                    let info = (server.addr.clone(), *port);
                    connector.add(move |proto| {
                        let info = info.clone();
                        async move { try_connect!(tcp => proto, info)?.map(Into::into) }
                    })
                }
            }
        }

        using_mux_connector = mux_connector;

        connector
    });

    let mut forwarder = if !using_mux_connector {
        PortForwarder::new(
            transport,
            service.target,
            TransmitterConnector::new(connector),
        )
    } else {
        let conf = MuxConfig::default();
        transport.send_packet(&conf.borrow_encode()?).await?;
        PortForwarder::new(
            transport,
            service.target,
            MuxTransmitterConnector::new(conf, connector),
        )
    };

    log::debug!("port forwarder started .");

    loop {
        let (linker, target) = forwarder.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = do_forward(linker, target).await {
                log::warn!("forward: {e:?}")
            };
        });
    }
}

async fn enter_proxy_service_main(
    config: Stateful<Config>,
    service: WithProxyService,
) -> error::Result<Rise> {
    let server = &config.server;
    let crypto = &server.crypto;
    let compress = &server.compress;

    let result = server
        .try_connect(|host, port| async move {
            // log::debug!("connect to server {host}:{port}");
            match host {
                Host::Ip(ip) => TcpStream::connect(SocketAddr::new(*ip, port)).await,
                Host::Domain(domain) => TcpStream::connect(format!("{domain}:{port}")).await,
            }
        })
        .await?
        .use_crypto(crypto.iter())
        .use_compress(compress.iter())
        .client_handshake::<TokioRuntime>(
            &server.authentication,
            Duration::from_secs(server.authentication_timeout as _),
        )
        .await;

    let transport = match result {
        Ok(stream) => stream,
        Err(e) => {
            log::error!("fail to handshake {e:?}");
            return Ok(Rise::Fatal);
        }
    };

    Ok(Rise::Restart)
}

async fn enter_bridge_service_main(
    config: Stateful<Config>,
    service: WithBridgeService,
) -> error::Result<Rise> {
    let mut listener = match TcpListener::bind(SocketAddr::new(service.bind, service.port)).await {
        Ok(listener) => listener,
        Err(e) => {
            return Ok(Rise::Fatal);
        }
    };

    log::debug!("bridge started ... {}:{}", service.bind, service.port);

    loop {
        let (addr, stream) = listener.accept().await?;

        log::debug!("{:?}", addr);

        let server = config.server.clone();

        tokio::spawn(async move {
            let result = server
                .try_connect(|host, port| async move {
                    match host {
                        Host::Ip(ip) => TcpStream::connect(SocketAddr::new(*ip, port)).await,
                        Host::Domain(domain) => {
                            TcpStream::connect(format!("{domain}:{port}")).await
                        }
                    }
                })
                .await;

            match result {
                Ok(upstream) => {
                    upstream.transfer(stream).await;
                }
                Err(e) => {}
            };
        });
    }
}

async fn do_forward(linker: Linker, target: FinalTarget) -> error::Result<()> {
    match target {
        FinalTarget::Udp { .. } => {}
        FinalTarget::Shell { path, args } => {
            let mut builder = fuso::pty::builder(path);

            builder.args(&args);

            match builder.build() {
                Err(e) => linker.cancel(e).await?,
                Ok(pty) => {
                    linker.link(Protocol::Tcp).await?.transfer(pty).await?;
                }
            }
        }
        FinalTarget::Tcp { addr, port } => {
            let result = addr
                .try_connect(port, |host, port| async move {
                    log::debug!("connect to {host}:{port}");
                    TcpStream::connect(format!("{host}:{port}")).await
                })
                .await;

            match result {
                Err(e) => linker.cancel(e).await?,
                Ok(stream) => {
                    linker.link(Protocol::Kcp).await?.transfer(stream).await?;
                }
            }
        }
        FinalTarget::Dynamic => {
            let transmitter = linker.link(Protocol::Tcp).await?;

            let udp = Arc::new(tokio::net::UdpSocket::bind("0.0.0.0:0").await?);

            let (writer, mut reader) = transmitter.split();

            loop {
                let pkt = reader.recv_packet().await?;

                let pkt: Fragment = pkt.decode()?;

                let udp = udp.clone();

                let mut writer = writer.clone();

                match pkt {
                    Fragment::UdpForward {
                        saddr,
                        daddr,
                        dport,
                        data,
                    } => {
                        let target = format!("{}:{dport}", daddr.to_string());
                        log::debug!("udp forward {saddr} -> {target}");

                        udp.send_to(&data, target).await?;

                        let mut buf = [0u8; 1400];

                        let _ =
                            tokio::time::timeout(std::time::Duration::from_secs(2), async move {
                                let (n, _) = udp.recv_from(&mut buf).await.unwrap();

                                let pkt = Fragment::Udp(saddr, buf[..n].to_vec()).encode().unwrap();
                                writer.send_packet(&pkt).await.unwrap();
                            })
                            .await;
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}
