use std::pin::Pin;

use futures::Future;

use crate::{connector::Connector, service::Service, Addr};

#[derive(Clone)]
pub struct TunProvider;

#[derive(Clone)]
pub struct TcpConnector;

// impl Service for TunProvider {
//     type Config = (super::SmolExecutor, Self::Socket);
//     type Socket = Addr;
//     type Stream = smol::net::TcpStream;
//     type Accepter = super::TcpListener;
//     type Connector = TcpConnector;

//     type Future = Pin<
//         Box<dyn Future<Output = crate::Result<(Self::Accepter, Self::Connector)>> + Send + 'static>,
//     >;

//     fn create(&self, config: &Self::Config) -> Self::Future {
//         let config = config.clone();
//         Box::pin(async move {
//             let (_, addr) = config;
//             let tcp = crate::TcpListener::bind("").await?;
//             Ok((tcp, TcpConnector))
//         })
//     }
// }

// impl Connector for TcpConnector {
//     type Socket = Addr;
//     type Stream = smol::net::TcpStream;

//     fn poll_connect(
//         self: Pin<&mut Self>,
//         cx: &mut std::task::Context<'_>,
//         socket: &Self::Socket,
//     ) -> std::task::Poll<crate::Result<Self::Stream>> {
//         unimplemented!()
//     }
// }
