pub mod ciphe;
pub mod client;
pub mod cmd;
pub mod core;
pub mod dispatch;
pub mod packet;
pub mod retain;
pub mod server;
pub mod handler;

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
mod tests {
    use std::sync::Arc;

    use fuso_api::Packet;

    use crate::{core::Config, dispatch::State, packet::Action};

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

    fn test_core() {
        smol::block_on(async move {
            

            // let io = crate::core::Fuso::bind(Config {
            //     bind_addr: "0.0.0.0".parse().unwrap(),
            //     debug: false,
            //     handler: vec![Arc::new(|cx|{
            //         async move{

            //             State::Next
            //         }
            //     })],
            // })
            // .await;
        });
    }
}
