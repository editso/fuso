use std::net::IpAddr;
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
use crate::{io, join};

use super::BoxedFuture;

macro_rules! async_connect {
    (local $writer: expr, $connector: expr, $id: expr, $socket: expr) => {{
        let mut writer = $writer.clone();
        async move {
            log::debug!("try to connect to {}", $socket);
            match $connector.call($socket).await {
                Ok(ok) => Ok(ok),
                Err(err) => {
                    let message = Message::MapError($id, err.to_string()).to_packet_vec();
                    return match writer.send_packet(&message).await {
                        Ok(_) => Err(err),
                        Err(err) => Err(err),
                    };
                }
            }
        }
    }};
    (remote $writer: expr, $connector: expr, $id: expr, $remote: expr, $local: expr) => {{
        let remote = $remote.clone();
        let local = $local.clone();
        let mut writer = $writer.clone();
        
        async move {
            log::debug!("try to connect to {}", remote);
            match $connector.call($remote).await {
                Ok(mut remote) => {
                    let message = Message::Map($id, local).to_packet_vec();
                    let remote = match remote.send_packet(&message).await {
                        Ok(()) => Ok(remote),
                        Err(err) => {
                            let message = Message::MapError($id, err.to_string()).to_packet_vec();
                            return match writer.send_packet(&message).await {
                                Ok(_) => Err(err),
                                Err(err) => Err(err),
                            };
                        }
                    };
                    match remote {
                        Ok(remote) => Ok(remote),
                        Err(err) => Err(err),
                    }
                }
                Err(err) => {
                    let message = Message::MapError($id, err.to_string()).to_packet_vec();
                    return match writer.send_packet(&message).await {
                        Ok(_) => Err(err),
                        Err(err) => Err(err),
                    };
                }
            }
        }
    }};
}

pub struct PenetrateClientFactory<C> {
    pub socket: (Socket, Socket),
    pub connector_factory: Arc<C>,
}

enum State {
    Ready(BoxedFuture<()>),
    Map(u32, Socket),
    Error(crate::Error),
}

pub struct PenetrateClient<CF, C, S> {
    socket: (Socket, Socket),
    reader: ReadHalf<S>,
    writer: WriteHalf<S>,
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
        let socket = self.socket.clone();
        let connector_factory = self.connector_factory.clone();

        Box::pin(async move {
            let mut stream = stream;
            let (remote, local) = socket;
            let message = Message::Bind(Bind::Bind(remote.clone())).to_packet_vec();

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
                Message::Bind(Bind::Bind(mut remote_bind)) => {
                    log::info!(
                        "The server created the listener successfully and bound to {}",
                        remote_bind
                    );

                    if remote_bind.is_ip_unspecified() {
                        remote_bind.from_set_host(&remote);
                    }

                    if remote_bind.is_ip_unspecified() {
                        remote_bind.set_ip([127, 0, 0, 1]);
                    }

                    Ok(PenetrateClient::new(
                        (remote_bind, local),
                        stream,
                        client_factory,
                        connector_factory,
                    ))
                }
                Message::Bind(Bind::Failed(socket, e)) => {
                    log::error!(
                        "an error occurred while creating the listener on the server addr={}, err={}",
                        socket,
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
        socket: (Socket, Socket),
        conn: S,
        client_factory: ClientFactory<CF>,
        connector_factory: Arc<C>,
    ) -> Self {
        let (reader, writer) = io::split(conn);

        let fut1 = Box::pin(Self::async_server_message(reader.clone()));
        let fut2 = Box::pin(Self::async_heartbeat(writer.clone()));

        Self {
            socket,
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
                Err(e) => return Ok(State::Error(e)),
            };

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
                Poll::Pending => futures.push(future),
                Poll::Ready(Ok(State::Error(e))) => {
                    log::warn!("server stops talking");
                    return Poll::Ready(Err(e));
                }
                Poll::Ready(Ok(State::Map(id, socket))) => {
                    let (remote, local) = self.socket.clone();
                    let s1_socket = remote;
                    let s2_socket = socket.default_or(local);
                    let s1_connector = self.client_factory.clone();
                    let s2_connector = self.connector_factory.clone();
                    let writer = self.writer.clone();

                    let server_fut =
                        { async_connect!(remote writer, s1_connector, id, s1_socket, s2_socket) };
                        
                    let client_fut = async_connect!(local writer, s2_connector, id, s2_socket);

                    let future = async move {
                        let (s1, s2) = join::join_output(server_fut, client_fut).await;
                        Ok(State::Ready({
                            match s2? {
                                Outcome::Stream(s2) => Box::pin(io::forward(s1?, s2)),
                                Outcome::Customize(s2) => s2.call(s1?),
                            }
                        }))
                    };

                    let fut1 = Box::pin(future);
                    let fut2 = Box::pin(Self::async_server_message(self.reader.clone()));

                    self.futures.push(fut1);
                    self.futures.push(fut2);
                }
                Poll::Ready(Ok(State::Ready(fut))) => {
                    poll_status = Poll::Ready(Ok(Some(fut)));
                    break;
                }
                Poll::Ready(Err(e)) => {
                    log::warn!("{:?}", e);
                }
            }
        }

        self.futures.extend(futures);

        poll_status
    }
}
