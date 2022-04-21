#[cfg(feature = "fuso-rt-tokio")]
#[tokio::main]
async fn main() {
    use std::sync::Arc;

    use fuso::{client, AsyncRecvPacket, ToBehavior};
    use tokio::net::TcpStream;

    let behavior = TcpStream::connect("addr")
        .await
        .unwrap()
        .recv_packet()
        .await
        .unwrap()
        .to_behavior()
        .unwrap();
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
