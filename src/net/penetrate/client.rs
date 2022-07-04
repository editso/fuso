use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use std::{future::Future, task::Poll};

use crate::io::{ReadHalf, WriteHalf};
use crate::{
    client::Outcome,
    generator::Generator,
    protocol::{AsyncRecvPacket, AsyncSendPacket, Bind, Message, ToPacket, TryToMessage},
    service::{ClientFactory, Factory},
    Kind, Socket, Stream,
};
use crate::{io, Addr};

use super::BoxedFuture;

pub struct PenetrateClientFactory<C> {
    pub connector_factory: Arc<C>,
}

enum State {
    Ready(BoxedFuture<()>),
    Map(u32, Socket),
    Error(crate::Error),
}

pub struct PenetrateClient<CF, C, S> {
    reader: ReadHalf<S>,
    writer: WriteHalf<S>,
    server: Addr,
    futures: Vec<BoxedFuture<State>>,
    client_factory: ClientFactory<CF>,
    connector_factory: Arc<C>,
}

impl<CF, C, S> Factory<(ClientFactory<CF>, S)> for PenetrateClientFactory<C>
where
    CF: Factory<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    C: Factory<Socket, Output = BoxedFuture<Outcome<S>>> + Send + Sync + 'static,
    S: Stream + Send + 'static,
{
    type Output = BoxedFuture<PenetrateClient<CF, C, S>>;

    fn call(&self, (client_factory, stream): (ClientFactory<CF>, S)) -> Self::Output {
        let connector_factory = self.connector_factory.clone();

        Box::pin(async move {
            let mut stream = stream;
            let message = Message::Bind(Bind::Bind(9999.into())).to_packet_vec();

            if let Err(e) = stream.send_packet(&message).await {
                log::error!("failed to send listen message to server err={}", e);
                return Err(e);
            }

            let message = match stream.recv_packet().await {
                Ok(packet) => packet.try_message(),
                Err(e) => {
                    log::error!("the listen message was sent successfully, but the server seems to have an error err={}", e);
                    return Err(e);
                }
            };

            if let Err(e) = message.as_ref() {
                log::error!("received a response from the server, but the message format is incorrect err={}", e);
                return Err(unsafe { message.unwrap_err_unchecked() });
            }

            let message = unsafe { message.unwrap_unchecked() };

            match message {
                Message::Bind(Bind::Bind(addr)) => {
                    log::info!(
                        "The server created the listener successfully and bound to {}",
                        addr
                    );
                    Ok(PenetrateClient::new(
                        addr,
                        stream,
                        client_factory,
                        connector_factory,
                    ))
                }
                Message::Bind(Bind::Failed(addr, e)) => {
                    log::error!(
                        "an error occurred while creating the listener on the server addr={}, err={}",
                        addr,
                        e
                    );
                    Err(Kind::Message(e).into())
                }
                message => {
                    log::error!(
                        "The message returned by the server cannot be accepted msg={}",
                        message
                    );
                    Err(Kind::Unexpected(format!("{}", message)).into())
                }
            }
        })
    }
}

impl<CF, C, S> PenetrateClient<CF, C, S>
where
    S: Stream + Send + 'static,
    C: 'static,
    CF: 'static,
{
    pub fn new(
        server: Addr,
        conn: S,
        client_factory: ClientFactory<CF>,
        connector_factory: Arc<C>,
    ) -> Self {
        let (reader, writer) = io::split(conn);

        let fut1 = Box::pin(Self::async_server_message(reader.clone()));
        let fut2 = Box::pin(Self::async_heartbeat(writer.clone()));

        Self {
            server,
            client_factory,
            connector_factory,
            reader: reader.clone(),
            writer: writer.clone(),
            futures: vec![fut1, fut2],
        }
    }

    async fn async_heartbeat(mut writer: WriteHalf<S>) -> crate::Result<State> {
        let ping = Message::Ping.to_packet_vec();

        loop {
            log::debug!("locked async_heartbeat");

            if let Err(e) = writer.send_packet(&ping).await {
                log::error!("failed to send heartbeat to server err={}", e);
                return Ok(State::Error(e));
            }

            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }

    async fn async_server_message(mut reader: ReadHalf<S>) -> crate::Result<State> {
        loop {
            let message = match reader.recv_packet().await {
                Ok(packet) => packet.try_message(),
                Err(e) => Err(e),
            };

            log::debug!("next message");

            if let Err(e) = message.as_ref() {
                log::warn!("server error {}", e);
                return Err(unsafe { message.unwrap_err_unchecked() });
            }

            let message = unsafe { message.unwrap_unchecked() };

            match message {
                Message::Map(id, socket) => {
                    break Ok(State::Map(id, socket));
                }
                message => {
                    log::debug!("received server message {:?}", message);
                }
            }
        }
    }
}

impl<CF, C, S> Generator for PenetrateClient<CF, C, S>
where
    CF: Factory<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    C: Factory<Socket, Output = BoxedFuture<Outcome<S>>> + Unpin + Send + Sync + 'static,
    S: Stream + Send + 'static,
{
    type Output = Option<BoxedFuture<()>>;
    fn poll_generate(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> std::task::Poll<crate::Result<Self::Output>> {
        let mut futures = Vec::new();
        let mut poll_status = Poll::Pending;

        while let Some(mut future) = self.futures.pop() {
            match Pin::new(&mut future).poll(cx) {
                Poll::Pending => {
                    futures.push(future)
                }
                Poll::Ready(Ok(State::Error(e))) => {
                    log::warn!("server stops talking");
                    return Poll::Ready(Err(e));
                }
                Poll::Ready(Ok(State::Map(id, socket))) => {
                    let client_factory = self.client_factory.clone();
                    let connector_factory = self.connector_factory.clone();
                    let server_addr = self.server.clone();
                    let mut writer = self.writer.clone();

                    let fut = Box::pin(async move {
                        log::debug!("connect to localhost");

                        let connected = connector_factory.call(socket.clone()).await;

                        if connected.is_err() {
                            let err = unsafe { connected.unwrap_err_unchecked() };
                            let message = Message::MapError(id, err.to_string()).to_packet_vec();
                            return match writer.send_packet(&message).await {
                                Ok(_) => Err(err),
                                Err(e) => Ok(State::Error(e)),
                            };
                        }

                        log::debug!("connect to remote");

                        let stream = client_factory
                            .call_connect(Socket::Tcp(Some(server_addr)))
                            .await;

                        if stream.is_err() {
                            let err = unsafe { stream.unwrap_err_unchecked() };
                            let message = Message::MapError(id, err.to_string()).to_packet_vec();
                            return match writer.send_packet(&message).await {
                                Ok(e) => Err(err),
                                Err(e) => Ok(State::Error(e)),
                            };
                        }

                        let mut s1 = unsafe { stream.unwrap_unchecked() };
                        let map_message = Message::Map(id, socket).to_packet_vec();

                        if let Err(err) = s1.send_packet(&map_message).await {
                            let message = Message::MapError(id, err.to_string()).to_packet_vec();
                            return match writer.send_packet(&message).await {
                                Ok(e) => Err(err),
                                Err(e) => Ok(State::Error(e)),
                            };
                        }

                        let connected = unsafe { connected.unwrap_unchecked() };

                        log::debug!("forward");
                        let future = match connected {
                            Outcome::Stream(s2) => Box::pin(async move {
                                io::forward(s1, s2).await;
                                Ok(())
                            }),
                            Outcome::Customize(factory) => factory.call(s1),
                        };

                        Ok(State::Ready(future))
                    });

                    let reader = self.reader.clone();

                    self.futures.push(fut);
                    self.futures
                        .push(Box::pin(Self::async_server_message(reader)));
                }
                Poll::Ready(Ok(State::Ready(fut))) => {
                    poll_status = Poll::Ready(Ok(Some(fut)));
                    break;
                }
                _ => unreachable!(),
            }
        }

        self.futures.extend(futures);

        poll_status
    }
}
