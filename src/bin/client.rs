use std::{sync::Arc, time::Duration};

use fuso::{
    join, penetrate::client::PenetrateClientFactory, Socket, TokioClientConnector,
    TokioClientForwardConnector,
};

fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .default_format()
        .format_module_path(false)
        .init();

    tokio::runtime::Runtime::new()
        .expect("failed to runtime")
        .block_on(async move {
            fuso::builder_client_with_tokio()
                .build(
                    Socket::Tcp(8888.into()),
                    PenetrateClientFactory {
                        connector_factory: Arc::new(TokioClientForwardConnector),
                        socket: {
                            (
                                Socket::Tcp(([0, 0, 0, 0], 9999).into()),
                                Socket::Tcp(([127, 0, 0, 1], 8080).into()),
                            )
                        },
                    },
                )
                .run()
                .await
        });
}

#[cfg(feature = "fuso-web")]
#[tokio::main]
async fn main() {}

#[cfg(feature = "fuso-api")]
#[tokio::main]
async fn main() {}

#[cfg(feature = "fuso-rt-smol")]
fn main() {}

#[cfg(feature = "fuso-rt-custom")]
fn main() {}
