use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use std::{future::Future, task::Poll};

use crate::io::{ReadHalf, WriteHalf};
use crate::{
    client::Mapper,
    generator::Generator,
    protocol::{AsyncRecvPacket, AsyncSendPacket, Bind, Message, ToPacket, TryToMessage},
    Kind, Socket, Stream, {ClientFactory, Factory},
};

use crate::{io, join, time};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

macro_rules! async_connect {
    ($writer: expr, $connector: expr, $id: expr, $socket: expr) => {{
        let socket = $socket.clone();
        let mut writer = $writer.clone();
        async move {
            log::debug!("try connect to {}", socket);
            match $connector.call(socket).await {
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
    C: Factory<Socket, Output = BoxedFuture<Mapper<S>>> + Send + Sync + 'static,
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
                    log::info!("the server is bound to {}", remote_bind);

                    if remote_bind.is_ip_unspecified() {
                        remote_bind.from_set_host(client_factory.default_socket());
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

        let fut1 = Box::pin(Self::register_server_handle(reader.clone()));
        let fut2 = Box::pin(Self::guard_server_heartbeat(writer.clone()));

        Self {
            socket,
            client_factory,
            connector_factory,
            reader: reader.clone(),
            writer: writer.clone(),
            futures: vec![fut1, fut2],
        }
    }

    async fn guard_server_heartbeat(mut writer: WriteHalf<S>) -> crate::Result<State> {
        let ping = Message::Ping.to_packet_vec();

        loop {
            if let Err(e) = writer.send_packet(&ping).await {
                log::error!("failed to send heartbeat to server err={}", e);
                return Ok(State::Error(e));
            }

            time::sleep(Duration::from_secs(10)).await;
        }
    }

    async fn register_server_handle(mut reader: ReadHalf<S>) -> crate::Result<State> {
        loop {
            let message = match reader.recv_packet().await {
                Ok(packet) => packet.try_message(),
                Err(e) => return Ok(State::Error(e)),
            };

            if let Err(e) = message.as_ref() {
                log::warn!("server error {}", e);
                return Ok(State::Error(unsafe { message.unwrap_err_unchecked() }));
            }

            let message = unsafe { message.unwrap_unchecked() };

            match message {
                Message::Map(id, socket) => {
                    break Ok(State::Map(id, socket));
                }
                message => {
                    log::trace!("received server message {:?}", message);
                }
            }
        }
    }
}

impl<CF, C, S> Generator for PenetrateClient<CF, C, S>
where
    CF: Factory<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    C: Factory<Socket, Output = BoxedFuture<Mapper<S>>> + Unpin + Send + Sync + 'static,
    S: Stream + Send + 'static,
{
    type Output = Option<BoxedFuture<()>>;
    fn poll_generate(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> std::task::Poll<crate::Result<Self::Output>> {
        let mut futures = std::mem::replace(&mut self.futures, Default::default());

        while let Some(mut future) = futures.pop() {
            match Pin::new(&mut future).poll(cx) {
                Poll::Pending => self.futures.push(future),
                Poll::Ready(Ok(State::Error(e))) => {
                    log::warn!("server stops talking");
                    return Poll::Ready(Err(e));
                }
                Poll::Ready(Ok(State::Map(id, socket))) => {
                    log::debug!("{}", socket);

                    let (remote, local) = self.socket.clone();

                    let s1_socket = remote
                        .if_stream_mixed(socket.is_mixed())
                        .with_kind(socket.kind());

                    let s2_socket = socket.default_or(local);
                    let s1_connector = self.client_factory.clone();
                    let s2_connector = self.connector_factory.clone();
                    let writer = self.writer.clone();

                    let server_fut = async_connect!(writer, s1_connector, id, s1_socket);
                    let client_fut = async_connect!(writer, s2_connector, id, s2_socket);

                    let future = async move {
                        let mut writer = writer;

                        let r = match time::wait_for(
                            Duration::from_secs(5),
                            join::join_output(server_fut, client_fut),
                        )
                        .await
                        {
                            Err(e) => Err(e),
                            Ok(r) => r,
                        };

                        let (mut s1, s2) = r?;

                        let message = Message::Map(id, s2_socket).to_packet_vec();

                        if let Err(e) = s1.send_packet(&message).await {
                            drop(s2);
                            let message = Message::MapError(id, e.to_string()).to_packet_vec();
                            if let Err(e) = writer.send_packet(&message).await {
                                return Ok(State::Error(e));
                            } else {
                                return Err(e);
                            }
                        }

                        Ok(State::Ready({
                            match s2 {
                                Mapper::Forward(s2) => Box::pin(io::forward(s1, s2)),
                                Mapper::Consume(s2) => s2.call(s1),
                            }
                        }))
                    };

                    let fut1 = Box::pin(future);
                    let fut2 = Box::pin(Self::register_server_handle(self.reader.clone()));

                    futures.push(fut1);
                    futures.push(fut2);
                }
                Poll::Ready(Ok(State::Ready(fut))) => {
                    self.futures.extend(futures);
                    return Poll::Ready(Ok(Some(fut)));
                }
                Poll::Ready(Err(e)) => {
                    log::warn!("{:?}", e);
                }
            }
        }

        log::debug!("{} futures remaining", self.futures.len());

        Poll::Pending
    }
}
