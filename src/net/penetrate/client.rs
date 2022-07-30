use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use std::{future::Future, task::Poll};

use crate::io::{ReadHalf, WriteHalf};
use crate::{
    client::Route,
    generator::Generator,
    protocol::{AsyncRecvPacket, AsyncSendPacket, Bind, Poto, ToBytes, TryToPoto},
    Kind, Socket, Stream, {ClientProvider, Provider},
};

use crate::{io, join, time, Address};

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
                    let poto = Poto::MapError($id, err.to_string()).bytes();
                    return match writer.send_packet(&poto).await {
                        Ok(_) => Err(err),
                        Err(err) => Err(err),
                    };
                }
            }
        }
    }};
}

macro_rules! default_socket {
    ($addr: expr, $default: expr) => {{
        if $addr.is_ip_unspecified() {
            $addr.from_set_host($default);
        }

        if $addr.is_ip_unspecified() {
            $addr.set_ip([127, 0, 0, 1]);
        }
    }};
}

pub struct PenetrateClientProvider<C> {
    pub transform: (Socket, Socket),
    pub connector_provider: Arc<C>,
}

enum State {
    Ready(BoxedFuture<()>),
    Map(u32, Socket),
    Error(crate::Error),
}

pub struct PenetrateClient<CF, C, S> {
    socket: (Address, Socket),
    reader: ReadHalf<S>,
    writer: WriteHalf<S>,
    futures: Vec<BoxedFuture<State>>,
    client_provider: ClientProvider<CF>,
    connector_provider: Arc<C>,
}

impl<CF, C, S> Provider<(ClientProvider<CF>, S)> for PenetrateClientProvider<C>
where
    CF: Provider<Address, Output = BoxedFuture<S>> + Send + Sync + 'static,
    C: Provider<Socket, Output = BoxedFuture<Route<S>>> + Send + Sync + 'static,
    S: Stream + Send + 'static,
{
    type Output = BoxedFuture<PenetrateClient<CF, C, S>>;

    fn call(&self, (client_provider, stream): (ClientProvider<CF>, S)) -> Self::Output {
        let socket = self.transform.clone();

        let connector_provider = self.connector_provider.clone();

        Box::pin(async move {
            let mut stream = stream;
            let (visit_addr, route_addr) = socket;
            let bind = Poto::Bind(Bind::Map(
                Socket::tcp(0).if_stream_mixed(true),
                visit_addr.clone(),
            ))
            .bytes();

            if let Err(e) = stream.send_packet(&bind).await {
                log::error!("failed to send listen message to server err={}", e);
                return Err(e);
            }

            let message = match stream.recv_packet().await {
                Ok(packet) => packet.try_poto(),
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
                Poto::Bind(Bind::Success(mut server_addr, mut visit_addr)) => {
                    default_socket!(visit_addr, client_provider.default_socket());
                    default_socket!(server_addr, client_provider.default_socket());

                    log::info!("the server is bound to {}", server_addr);
                    log::info!("please visit {}", visit_addr);

                    Ok(PenetrateClient::new(
                        (server_addr, route_addr),
                        stream,
                        client_provider,
                        connector_provider,
                    ))
                }
                Poto::Bind(Bind::Failed(fail)) => {
                    log::error!(
                        "an error occurred while creating the listener on the server {}",
                        fail,
                    );
                    Err(Kind::Message(fail).into())
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
        socket: (Address, Socket),
        conn: S,
        client_provider: ClientProvider<CF>,
        connector_provider: Arc<C>,
    ) -> Self {
        let (reader, writer) = io::split(conn);

        let fut1 = Box::pin(Self::register_server_handle(reader.clone()));
        let fut2 = Box::pin(Self::guard_server_heartbeat(writer.clone()));

        Self {
            socket,
            client_provider,
            connector_provider,
            reader: reader.clone(),
            writer: writer.clone(),
            futures: vec![fut1, fut2],
        }
    }

    async fn guard_server_heartbeat(mut writer: WriteHalf<S>) -> crate::Result<State> {
        let ping = Poto::Ping.bytes();

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
                Ok(packet) => packet.try_poto(),
                Err(e) => return Ok(State::Error(e)),
            };

            if let Err(e) = message.as_ref() {
                log::warn!("server error {}", e);
                return Ok(State::Error(unsafe { message.unwrap_err_unchecked() }));
            }

            let message = unsafe { message.unwrap_unchecked() };

            match message {
                Poto::Map(id, socket) => {
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
    CF: Provider<Address, Output = BoxedFuture<S>> + Send + Sync + 'static,
    C: Provider<Socket, Output = BoxedFuture<Route<S>>> + Unpin + Send + Sync + 'static,
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
                    let s1_connector = self.client_provider.clone();
                    let s2_connector = self.connector_provider.clone();
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

                        let message = Poto::Map(id, s2_socket).bytes();

                        if let Err(e) = s1.send_packet(&message).await {
                            drop(s2);
                            let message = Poto::MapError(id, e.to_string()).bytes();
                            if let Err(e) = writer.send_packet(&message).await {
                                return Ok(State::Error(e));
                            } else {
                                return Err(e);
                            }
                        }

                        Ok(State::Ready({
                            match s2 {
                                Route::Forward(s2) => Box::pin(io::forward(s1, s2)),
                                Route::Provider(s2) => s2.call(s1),
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
