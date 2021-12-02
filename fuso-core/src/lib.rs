pub mod ciphe;
pub mod client;
pub mod cmd;
pub mod core;
pub mod dispatch;
pub mod handler;
pub mod packet;
pub mod retain;
pub mod server;

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
        packet::Action,
    };

    fn init_logger() {
        env_logger::builder()
            .filter_level(log::LevelFilter::Debug)
            .init();
    }

    #[test]
    fn test_packet() {
        init_logger();

        let action = Action::Bind(Some("127.0.0.1:8080".parse().unwrap()));

        let packet: Packet = action.into();

        log::debug!("{:?}", packet);

        let action: fuso_api::Result<Action> = packet.try_into();

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
                .with_chain(|chain| {
                    chain.next(|mut tcp, cx| async move {
                        let action: Action = tcp.recv().await?.try_into()?;
                        match action {
                            Action::Bind(addr) => {
                                let conv = cx.spwan(tcp.into(), addr).await?;
                                log::debug!("[fuso] accept {}", conv);
                                Ok(State::Accept(()))
                            }
                            Action::Connect(conv) => {
                                cx.route(conv, tcp.into()).await?;
                                Ok(State::Accept(()))
                            }
                            _ => Ok(State::Next),
                        }
                    })
                })
                .chain_strategy(|chain| {
                    chain
                        .next(|tcp, cx| async move {
                            Ok(State::Next)
                        })
                        .next(|tcp, cx| async move {
                            log::debug!("strategy");
                            Ok(State::Next)
                        })
                })
                .build()
                .await
                .unwrap();

            loop {
                match fuso.accept().await {
                    Ok(fuso) => {
                        log::debug!("mut .... ");
                    }
                    Err(_) => {}
                }
            }
        });
    }
}
