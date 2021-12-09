pub mod bridge;
pub mod ciphe;
pub mod client;
pub mod cmd;
pub mod core;
pub mod dispatch;
pub mod handler;
pub mod packet;
pub mod retain;
pub mod udp;

use std::sync::Arc;

pub use fuso_api::*;
use futures::{AsyncRead, AsyncReadExt, AsyncWrite};
use smol::lock::Mutex;

#[inline]
pub fn split<T>(o: T) -> (T, T)
where
    T: Clone,
{
    (o.clone(), o)
}

#[inline]
pub fn split_mutex<T>(o: T) -> (Arc<Mutex<T>>, Arc<Mutex<T>>) {
    let mutex = Arc::new(Mutex::new(o));
    split(mutex)
}

pub async fn forward<I, O>(origin: I, target: O) -> std::io::Result<()>
where
    I: AsyncWrite + AsyncRead + Unpin + Send + Sync + 'static,
    O: AsyncWrite + AsyncRead + Unpin + Send + Sync + 'static,
{
    let (reader, writer) = origin.split();
    let (reader_t, write_t) = target.split();

    smol::future::race(
        smol::io::copy(reader, write_t),
        smol::io::copy(reader_t, writer),
    )
    .await?;

    Ok(())
}

#[cfg(test)]
#[allow(unused)]
mod tests {
    use std::sync::Arc;

    use fuso_api::{FusoListener, FusoPacket, Packet};
    use futures::{AsyncReadExt, AsyncWriteExt};
    use smol::net::TcpStream;

    use crate::{
        core::{self, Config},
        dispatch::State,
        handler::ChainHandler,
        packet::{Action, Addr},
    };

    fn init_logger() {
        env_logger::builder()
            .filter_level(log::LevelFilter::Debug)
            .init();
    }

    #[test]
    fn test_packet() {
        init_logger();

        let action = Action::Bind(
            Some("hello world".into()),
            Some("127.0.0.1:80".parse().unwrap()),
        );

        let packet: Packet = action.into();

        log::debug!("{:?}", packet);

        let action: fuso_api::Result<Action> = packet.try_into();

        log::debug!("{:?}", action);

        let action = Action::UdpRequest(
            10,
            Addr::Domain("baidu.com".into(), 80),
            b"hello world".to_vec(),
        );

        let packet: Packet = action.into();

        log::debug!("{:?}", packet.encode());

        let bytes = b"\0\0\0\xccp\0\0\0(\0\0\0\0\0\0\0\0\n\0\0\0\r\x03\tbaidu.com\0P\0\0\0\x0bhello world1111";

        let action: Action = Packet::decode_data(bytes).unwrap().try_into().unwrap();

        log::debug!("{:?}", action);

        let action = Action::UdpRespose(10, b"hello world".to_vec());

        let packet: Packet = action.into();

        log::debug!("{:?}", packet.encode());

        let bytes = b"\0\0\0\xccq\0\0\0\x17\0\0\0\0\0\0\0\0\n\0\0\0\x0bhello world11111";

        let action: Action = Packet::decode_data(bytes).unwrap().try_into().unwrap();

        log::debug!("{:?}", action);
    }

    #[test]
    #[allow(unused)]
    fn test_core() {
        init_logger();
        smol::block_on(async move {
            let builder = crate::core::Fuso::builder();

            let mut fuso = builder
                .with_config(Config {
                    debug: false,
                    bind_addr: "127.0.0.1:9999".parse().unwrap(),
                })
                .chain_handler(|chain| {
                    chain.next(|mut tcp, cx| async move {
                        let action: Action = tcp.recv().await?.try_into()?;

                        match action {
                            Action::Bind(name, addr) => {
                                let client_addr = tcp.peer_addr().unwrap();
                                let conv = cx.spwan(tcp, addr, name).await?;
                                log::debug!(
                                    "[fuso] accept conv={}, addr={}, name={}",
                                    conv,
                                    client_addr,
                                    conv
                                );
                                Ok(State::Accept(()))
                            }
                            Action::Connect(conv, id) => {
                                cx.route(conv, id, tcp.into()).await?;
                                Ok(State::Accept(()))
                            }
                            _ => Ok(State::Next),
                        }
                    })
                })
                .chain_strategy(|chain| {
                    chain.next(|tcp, cx| async move { Ok(State::Next) }).next(
                        |tcp, cx| async move {
                            log::debug!("strategy");
                            Ok(State::Next)
                        },
                    )
                })
                .build()
                .await
                .unwrap();

            loop {
                match fuso.accept().await {
                    Ok(fuso) => {
                        log::debug!("..");
                    }
                    Err(_) => {}
                }
            }
        });
    }
}
