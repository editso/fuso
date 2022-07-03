use std::{sync::Arc, time::Duration};

use async_mutex::Mutex;

use crate::{
    client::Outcome,
    generator::{Generate, Generator},
    io,
    join::Join,
    protocol::{AsyncRecvPacket, AsyncSendPacket, Bind, Message, ToPacket},
    service::{ClientFactory, Factory},
    Socket, Stream,
};

use super::BoxedFuture;

pub struct PenetrateClientFactory {}

pub struct PenetrateClient {}

impl<CF, S> Factory<(ClientFactory<CF>, S)> for PenetrateClientFactory
where
    CF: Factory<Socket, Output = BoxedFuture<Outcome<S>>>,
    S: Stream + Send + 'static,
{
    type Output = BoxedFuture<PenetrateClient>;

    fn call(&self, (_, mut stream): (ClientFactory<CF>, S)) -> Self::Output {
        Box::pin(async move {
            let message = Message::Bind(Bind::Bind(9999.into())).to_packet_vec();
            let (mut reader, mut writer) = io::split(stream);

            writer.send_packet(&message).await?;

            let f1 = {
                async move {
                    loop {
                        log::debug!("recv packet ...");
                        match reader.recv_packet().await {
                            Ok(packet) => {
                                log::debug!("{:?}", packet);
                            }
                            Err(_) => break,
                        }
                    }
                }
            };

            let f2 = async move {
                loop {
                    let e = writer.send_packet(&Message::Ping.to_packet_vec()).await;
                    if e.is_err() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            };

            tokio::spawn(Join::join(f1, f2)).await;

            unimplemented!()
        })
    }
}

impl Generator for PenetrateClient {
    type Output = Option<BoxedFuture<()>>;
    fn poll_generate(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> std::task::Poll<crate::Result<Self::Output>> {
        unimplemented!()
    }
}
