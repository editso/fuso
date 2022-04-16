#[cfg(feature = "fuso-rt-tokio")]
#[tokio::main]
async fn main() {
    use fuso::AsyncRecvPacket;
    use tokio::net::TcpListener;

    let tcp = TcpListener::bind("0.0.0.0:80").await.unwrap();

    loop {
        let (mut tcp, addr) = tcp.accept().await.unwrap();

        let a = tcp.recv_packet().await;

        
        
        

    }
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
