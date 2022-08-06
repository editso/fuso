use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use std::{future::Future, task::Poll};

use serde::{Deserialize, Serialize};

use crate::io::{ReadHalf, WriteHalf};
use crate::protocol::IntoPacket;
use crate::{
    client::Route,
    generator::Generator,
    protocol::{AsyncRecvPacket, AsyncSendPacket, Bind, Poto, ToBytes, TryToPoto},
    Kind, Socket, Stream, {ClientProvider, Provider},
};

use crate::{io, join, time, Address, Processor, Platform};

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


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// 服务名
    pub(super) name: String,
    /// 创建连接等待时间, 超过视为超时
    pub(super) maximum_wait: Duration,
    /// 心跳延时
    pub(super) heartbeat_delay: Duration,
    /// 是否启用 kcp
    pub(super) enable_kcp: bool,
    /// 是否启用socks5
    pub(super) enable_socks5: bool,
    /// socks5用户名
    pub(super) socks_username: Option<String>,
    /// socks5密码
    pub(super) socks_password: Option<String>,
    /// 是否启用socks5 udp转发
    pub(super) enable_socks5_udp: bool,
    pub(super) version: String,
    pub(super) platform: Platform
}

pub struct PenetrateClientProvider<C> {
    pub config: Config,
    pub forward: (Socket, Socket),
    pub connector_provider: Arc<C>,
}

enum State {
    Leave(Socket),
    Ready(BoxedFuture<()>),
    Map(u32, Socket),
    Error(crate::Error),
}

pub struct PenetrateClient<P, C, S> {
    reader: ReadHalf<S>,
    config: Config,
    forward: (Address, Socket),
    writer: WriteHalf<S>,
    futures: Vec<BoxedFuture<State>>,
    processor: Processor<ClientProvider<P>, S, ()>,
    connector_provider: Arc<C>,
}

impl<P, C, S> Provider<(S, Processor<ClientProvider<P>, S, ()>)> for PenetrateClientProvider<C>
where
    P: Provider<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    C: Provider<Socket, Output = BoxedFuture<Route<S>>> + Send + Sync + 'static,
    S: Stream + Send + 'static,
{
    type Output = BoxedFuture<PenetrateClient<P, C, S>>;

    fn call(&self, (stream, processor): (S, Processor<ClientProvider<P>, S, ()>)) -> Self::Output {
        let socket = self.forward.clone();
        let config = self.config.clone();

        let connector_provider = self.connector_provider.clone();

        Box::pin(async move {
            let mut stream = stream;
            let (visit_addr, route_addr) = socket;
            let bind = Poto::Bind(Bind::Setup(
                Socket::tcp(0).if_stream_mixed(config.enable_kcp || config.enable_socks5_udp),
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
                    let copy_cfg = config.clone();
                    let config_packet = config.into_packet();
                    let config_bytes = config_packet.encode();

                    if let Err(e) = stream.send_packet(&config_bytes).await {
                        log::warn!("server error {}", e);
                        return Err(e);
                    };

                    let configured = stream.recv_packet().await;

                    if let Err(e) = configured.as_ref() {
                        log::warn!("server error {}", e);
                        return Err(unsafe { configured.unwrap_err_unchecked() });
                    }

                    let configured = unsafe { configured.unwrap_unchecked() };
                    let configured = bincode::deserialize::<String>(&configured.payload);
                    if let Err(e) = configured.as_ref() {
                        log::warn!("server configuration error {}", e);
                        return Err(e.to_string().into());
                    };

                    let configured = unsafe { configured.unwrap_unchecked() };

                    if configured.ne("YES") {
                        log::warn!("server configuration error {}", configured);
                        return Err(configured.into());
                    };

                    default_socket!(visit_addr, processor.default_socket());
                    default_socket!(server_addr, processor.default_socket());

                    log::info!("the server is bound to {}", server_addr);
                    log::info!("please visit {}", visit_addr);

                    Ok(PenetrateClient::new(
                        (server_addr, route_addr),
                        stream,
                        copy_cfg,
                        processor,
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

impl<P, C, S> PenetrateClient<P, C, S>
where
    P: Provider<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    C: Provider<Socket, Output = BoxedFuture<Route<S>>> + Send + Sync + 'static,
    S: Stream + Send + 'static,
{
    pub fn new(
        socket: (Address, Socket),
        conn: S,
        config: Config,
        processor: Processor<ClientProvider<P>, S, ()>,
        connector_provider: Arc<C>,
    ) -> Self {
        let (reader, writer) = io::split(conn);

        let fut1 = Box::pin(Self::register_server_handle(reader.clone()));
        let fut2 = Box::pin(Self::guard_server_heartbeat(
            writer.clone(),
            config.maximum_wait,
        ));

        Self {
            forward: socket,
            processor,
            config,
            connector_provider,
            reader: reader.clone(),
            writer: writer.clone(),
            futures: vec![fut1, fut2],
        }
    }

    async fn guard_server_heartbeat(
        mut writer: WriteHalf<S>,
        timeout: Duration,
    ) -> crate::Result<State> {
        let ping = Poto::Ping.bytes();

        loop {
            if let Err(e) = writer.send_packet(&ping).await {
                log::error!("failed to send heartbeat to server err={}", e);
                return Ok(State::Error(e));
            }

            time::sleep(timeout).await;
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

    fn start_async_forward(
        &self,
        id: u32,
        server_socket: Socket,
        target_socket: Socket,
    ) -> BoxedFuture<State> {
        let s1_connector = self.processor.clone();
        let s2_connector = self.connector_provider.clone();
        let maximum_wait = self.config.maximum_wait.clone();

        let server_fut = async_connect!(self.writer, s1_connector, id, server_socket);
        let client_fut = async_connect!(self.writer, s2_connector, id, target_socket);
        let server_writer = self.writer.clone();
        let processor = self.processor.clone();

        let future = async move {
            let mut server_writer = server_writer;
            let result =
                time::wait_for(maximum_wait, join::join_output(server_fut, client_fut)).await;

            let result = match result {
                Err(e) => Err(e),
                Ok(r) => r,
            };

            let (s1, s2) = result?;

            let mut s1 = processor.decorate(s1).await?;

            let poto = Poto::Map(id, target_socket).bytes();

            if let Err(e) = s1.send_packet(&poto).await {
                let message = Poto::MapError(id, e.to_string()).bytes();
                if let Err(e) = server_writer.send_packet(&message).await {
                    Ok(State::Error(e))
                } else {
                    Err(e)
                }
            } else {
                Ok(State::Ready({
                    match s2 {
                        Route::Forward(s2) => Box::pin(io::forward(s1, s2)),
                        Route::Provider(s2) => s2.call(s1),
                    }
                }))
            }
        };

        Box::pin(future)
    }
}

impl<CF, C, S> Generator for PenetrateClient<CF, C, S>
where
    CF: Provider<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    C: Provider<Socket, Output = BoxedFuture<Route<S>>> + Send + Sync + 'static,
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
                Poll::Ready(Ok(State::Leave(socket))) => {
                    log::warn!("leave {}", socket);
                }
                Poll::Ready(Ok(State::Map(id, target_socket))) => {
                    log::debug!("{}", target_socket);

                    let (server, local) = self.forward.clone();
                    let target_socket = target_socket.default_or(local);
                    let server_writer = self.writer.clone();

                    let future = match server.select(&target_socket) {
                        Ok(server_socket) => {
                            self.start_async_forward(id, server_socket, target_socket)
                        }
                        Err(e) => Box::pin(async move {
                            let mut server_writer = server_writer;
                            let poto = Poto::MapError(id, e.to_string()).bytes();
                            match server_writer.send_packet(&poto).await {
                                Ok(()) => Ok(State::Leave(target_socket)),
                                Err(e) => Ok(State::Error(e)),
                            }
                        }),
                    };

                    let fut2 = Box::pin(Self::register_server_handle(self.reader.clone()));

                    futures.push(future);
                    futures.push(fut2);
                }
                Poll::Ready(Ok(State::Ready(fut))) => {
                    self.futures.extend(futures);
                    return Poll::Ready(Ok(Some(fut)));
                }
                Poll::Ready(Err(e)) => {
                    log::trace!("{:?}", e);
                }
            }
        }

        log::debug!("{} futures remaining", self.futures.len());

        Poll::Pending
    }
}
