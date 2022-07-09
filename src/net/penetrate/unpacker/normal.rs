use std::pin::Pin;

use crate::{
    guard::Fallback,
    penetrate::{
        server::{Peer, Visitor},
        Adapter,
    },
    protocol::{AsyncRecvPacket, Message, TryToMessage},
    Factory,
    Socket, Stream,
};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub struct NormalUnpacker;


impl<S> Factory<Fallback<S>> for NormalUnpacker
where
    S: Stream + Send + 'static,
{
    type Output = BoxedFuture<Adapter<S>>;

    fn call(&self, stream: Fallback<S>) -> Self::Output {
        Box::pin(async move {
            log::debug!("call_normal");
            let mut stream = stream;
            match stream.recv_packet().await {
                Ok(packet) => match packet.try_message() {
                    Err(_) => Ok(Adapter::Reject(stream)),
                    Ok(message) => match message {
                        Message::Map(id, socket) => {
                            log::debug!("client establishes mapping to {}", socket);
                            Ok(Adapter::Accept(Peer::Mapper(id, stream)))
                        }
                        _ => Ok(Adapter::Reject(stream)),
                    },
                },
                Err(e) => {
                    if !e.is_packet_err() {
                        Ok(Adapter::Reject(stream))
                    } else {
                        log::debug!("need to notify the client to create a mapping");
                        Ok(Adapter::Accept(Peer::Visitor(
                            Visitor::Forward(stream),
                            Socket::default(),
                        )))
                    }
                }
            }
        })
    }
}
