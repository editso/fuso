mod accepter;
mod connection;
mod handshake;
mod preprocessor;
mod transport;

pub use accepter::*;
pub use handshake::*;
pub use preprocessor::*;

use parking_lot::Mutex;
use std::future::Future;

use std::marker::PhantomData;
use std::{collections::HashMap, pin::Pin, sync::Arc, task::Poll};

use crate::core::future::{Poller, Select};
use crate::core::io::AsyncReadExt;
use crate::core::rpc::structs::port_forward::{self, Target};
use crate::core::rpc::AsyncCall;
use crate::core::task::{setter, Getter, Setter};
use crate::core::token::IncToken;
use crate::core::Stream;
use crate::runtime::Runtime;
use crate::{
    core::{
        accepter::Accepter,
        io::{AsyncRead, AsyncWrite},
        processor::{AbstractPreprocessor, Preprocessor},
        rpc::structs::port_forward::{Request, VisitorProtocol},
    },
    error,
};

use transport::Transport;

use self::connection::UdpConnections;

type Connection = crate::core::Connection<'static>;

enum Outcome {
    Ready(u64, Connection),
    Mapped(u64),
    Pending(u64, Getter<()>),
    Timeout(u64),
    Complete(Connection, Connection),
    Transport(u64, error::FusoError),
    Stopped(Option<error::FusoError>),
}

#[derive(Debug, Clone, Copy)]
enum ConnKind {
    Udp,
    Tcp,
}

struct StashedConn {
    kind: ConnKind,
    conn: Connection,
    #[allow(unused)]
    guard: Setter<()>,
}

pub enum Whence {
    Visitor(Connection),
    Mapping(Connection),
}

#[derive(Clone)]
pub struct Visitors {
    inc_token: IncToken,
    connections: Arc<Mutex<HashMap<u64, StashedConn>>>,
}

pub struct PortForwarder<Runtime, A, T> {
    poller: Poller<'static, error::Result<Outcome>>,
    uconns: UdpConnections<Runtime>,
    accepter: A,
    visitors: Visitors,
    transport: Transport<T>,
    prepmap: AbstractPreprocessor<'static, Connection, error::Result<Connection>>,
    prepvis: AbstractPreprocessor<'static, Connection, error::Result<VisitorProtocol>>,
    _marked: PhantomData<Runtime>,
}

impl<R, A, T> Accepter for PortForwarder<R, A, T>
where
    A: Accepter<Output = Whence> + Unpin + 'static,
    T: Stream + Unpin + Send + 'static,
    R: Runtime + Sync + Send + Unpin + 'static,
{
    type Output = (Connection, Connection);

    fn poll_accept(
        mut self: std::pin::Pin<&mut Self>,
        ctx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::error::Result<Self::Output>> {
        let mut polled = false;

        while !polled {
            let whence = match Pin::new(&mut self.accepter).poll_accept(ctx)? {
                std::task::Poll::Pending => None,
                std::task::Poll::Ready(whence) => Some(whence),
            };

            polled = match whence {
                None => true,
                Some(Whence::Visitor(conn)) => {
                    let fut = Self::do_prepare_visitor(
                        conn,
                        self.uconns.clone(),
                        self.visitors.clone(),
                        self.transport.clone(),
                        self.prepvis.clone(),
                    );

                    self.poller.add(fut);

                    false
                }
                Some(Whence::Mapping(conn)) => {
                    let fut = Self::do_prepare_mapping(
                        conn,
                        self.uconns.clone(),
                        self.visitors.clone(),
                        self.transport.clone(),
                        self.prepmap.clone(),
                    );

                    self.poller.add(fut);

                    false
                }
            };

            let (need_poll, outcome) = match Pin::new(&mut self.poller).poll(ctx) {
                Poll::Pending => continue,
                Poll::Ready(Ok(o)) => (false, o),
                Poll::Ready(Err(e)) => {
                    log::error!("{:?}", e);
                    continue;
                }
            };

            if let Poll::Ready(()) = Pin::new(&mut self.uconns).poll(ctx)? {
                return Poll::Ready(Err(error::FusoError::Abort));
            }

            polled = need_poll;

            match outcome {
                Outcome::Mapped(token) => match self.visitors.take(token) {
                    None => {
                        log::trace!("mapping successful {{ token={token} }}")
                    }
                    Some(_) => {
                        log::debug!("mapping error {{ token={token} }}")
                    }
                },
                Outcome::Complete(conn, trans) => {
                    return Poll::Ready(Ok((conn, trans)));
                }
                Outcome::Pending(token, getter) => {
                    if self.visitors.exists(token) {
                        self.poller.add(Self::wait_transport(
                            token,
                            getter,
                            std::time::Duration::from_secs(10),
                        ));
                    }
                }
                Outcome::Timeout(token) => {
                    if let Some(conn) = self.visitors.take(token) {
                        log::warn!(
                            "failed to create mapping {{ token={}, addr={}, reason='Timeout' }}",
                            token,
                            conn.addr()
                        );
                    }
                }
                Outcome::Ready(token, transport) => {
                    match self.visitors.take(token) {
                        Some(visitor) => return Poll::Ready(Ok((visitor, transport))),
                        None => log::warn!("invalid mapping because the client has been closed {{ token={token} }}")
                    };
                }
                Outcome::Stopped(error) => {
                    return {
                        match error {
                            Some(e) => Poll::Ready(Err(e)),
                            None => Poll::Ready(Err(error::FusoError::Abort)),
                        }
                    }
                }
                Outcome::Transport(token, transport) => {
                    if let Some(conn) = self.visitors.take(token) {
                        log::warn!(
                            "failed to create mapping {{ token={}, addr={}, reason='{}' }}",
                            token,
                            conn.addr(),
                            transport
                        );
                    }
                }
            };
        }

        Poll::Pending
    }
}

#[cfg(feature = "fuso-runtime")]
impl<R, A, T> PortForwarder<R, A, T>
where
    A: Accepter<Output = Whence> + Unpin + 'static,
    T: Stream + Unpin + Send + 'static,
    R: Runtime + 'static,
{
    pub fn new_with_runtime<P, M>(transport: T, accepter: A, prepvis: P, prepmap: M) -> Self
    where
        P: Into<AbstractPreprocessor<'static, Connection, error::Result<VisitorProtocol>>>,
        M: Preprocessor<Connection, Output = error::Result<Connection>>,
        M: Send + Sync + 'static,
    {
        let mut poller = Poller::new();

        let (transport, hold) = Transport::new::<R>(std::time::Duration::from_secs(10), transport);

        poller.add(async move {
            match hold.await {
                Ok(()) => Ok(Outcome::Stopped(None)),
                Err(e) => Ok(Outcome::Stopped(Some(e))),
            }
        });

        let uconns = UdpConnections::default();

        Self {
            poller,
            uconns,
            accepter,
            transport,
            visitors: Default::default(),
            prepvis: prepvis.into(),
            prepmap: AbstractPreprocessor(Arc::new(prepmap)),
            _marked: PhantomData,
        }
    }
}

impl<R, A, T> PortForwarder<R, A, T>
where
    R: Runtime,
    T: AsyncWrite + AsyncRead + Send + Unpin,
{
    async fn wait_transport(
        token: u64,
        getter: Getter<()>,
        timeout: std::time::Duration,
    ) -> error::Result<Outcome> {
        log::trace!("wait for mapping {{ token={token}, timeout={timeout:?} }}");

        let mut select = Select::new();

        select.add(async move {
            R::sleep(timeout).await;
            Ok(Outcome::Timeout(token))
        });

        select.add(async move {
            let _ = getter.await;
            Ok(Outcome::Mapped(token))
        });

        select.await
    }

    async fn do_prepare_visitor(
        conn: Connection,
        uconns: UdpConnections<R>,
        visitors: Visitors,
        transport: Transport<T>,
        preprocessor: AbstractPreprocessor<'_, Connection, error::Result<VisitorProtocol>>,
    ) -> error::Result<Outcome> {
        let mut conn_kind = ConnKind::Tcp;
        let mut udp_forward = false;
        let mut transport = transport;

        let (conn, addr) = match preprocessor.prepare(conn).await? {
            VisitorProtocol::Socks(conn, target) => (conn, Some(target)),
            VisitorProtocol::Other(conn, addr) => match addr {
                None => (conn, None),
                Some(target) => (conn, Some(target)),
            },
        };

        if let Some(target) = addr.as_ref() {
            if let Target::Udp(_, _) = target {
                conn_kind = ConnKind::Udp;
                udp_forward = true;
            }
        }

        if udp_forward {
            if let Some(transport) = uconns.apply_idle_conn().await? {
                return Ok(Outcome::Complete(conn, transport));
            }
        }

        log::trace!("create mapping {} -- [T] -- {:?}", conn.addr(), addr);

        let (setter, getter) = setter();

        let token = visitors.store(conn_kind, conn, setter);

        let request = match udp_forward {
            true => Request::Dyn(token, addr),
            false => Request::New(token, addr),
        };

        let result = R::wait_for(std::time::Duration::from_secs(10), async move {
            match transport.call(request).await {
                Err(e) => Err(e),
                Ok(resp) => match resp {
                    port_forward::Response::Ok => Ok(()),
                    port_forward::Response::Error(msg) => Err(msg.into()),
                    port_forward::Response::Cancel => Err(error::FusoError::Cancel),
                },
            }
        })
        .await;

        match result {
            Err(_) => Ok(Outcome::Transport(token, error::FusoError::NotResponse)),
            Ok(r) => match r {
                Ok(_) => Ok(Outcome::Pending(token, getter)),
                Err(e) => Ok(Outcome::Transport(token, e)),
            },
        }
    }

    async fn do_prepare_mapping(
        conn: Connection,
        uconns: UdpConnections<R>,
        visitors: Visitors,
        _: Transport<T>,
        preprocessor: AbstractPreprocessor<'_, Connection, error::Result<Connection>>,
    ) -> error::Result<Outcome> {
        let mut conn = preprocessor.prepare(conn).await?;

        let mut buf = [0u8; 8];

        conn.read_exact(&mut buf).await?;

        let token = u64::from_be_bytes(buf);

        if let Some(ConnKind::Udp) = visitors.kind(token) {
            conn = uconns.exchange(conn).await?;
        };

        log::trace!("created mapping {{ token={token} }}");

        Ok(Outcome::Ready(token, conn))
    }
}

impl Default for Visitors {
    fn default() -> Self {
        Self {
            inc_token: Default::default(),
            connections: Default::default(),
        }
    }
}

impl Visitors {
    fn exists(&self, token: u64) -> bool {
        self.connections.lock().contains_key(&token)
    }

    fn take(&self, token: u64) -> Option<Connection> {
        self.connections.lock().remove(&token).map(|sc| sc.conn)
    }

    fn kind(&self, token: u64) -> Option<ConnKind> {
        self.connections.lock().get(&token).map(|sc| sc.kind)
    }

    fn store(&self, kind: ConnKind, conn: Connection, guard: Setter<()>) -> u64 {
        let token = self.next_token();

        self.connections
            .lock()
            .insert(token, StashedConn { kind, conn, guard });

        token
    }

    fn next_token(&self) -> u64 {
        let connections = self.connections.lock();
        self.inc_token
            .next(|token| !connections.contains_key(&token))
    }
}
