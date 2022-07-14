mod third_party;

pub use third_party::KcpErr;

mod kcp;
pub use kcp::*;

mod builder;
pub use builder::*;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{ext::{AsyncReadExt, AsyncWriteExt}, AccepterExt};

    use super::{KcpListener, KcpConnector, Increment};

    fn init_logger() {
        env_logger::builder()
            .filter_module("fuso",log::LevelFilter::Debug)
            .init();
    }

  
    #[test]
    pub fn test_kcp_server() {
        init_logger();

        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let udp = tokio::net::UdpSocket::bind("0.0.0.0:7777").await.unwrap();
                let udp = Arc::new(udp);

                let mut kcp = KcpListener::bind(udp).unwrap();

                loop {
                    match kcp.accept().await {
                        Ok(mut kcp) => { 
                            tokio::spawn(async move{
                                loop {  
                                    let mut buf = Vec::new();
                                    buf.resize(1500, 0);
                                    let n = kcp.read(&mut buf).await.unwrap();
                                    buf.truncate(n);
                                    let _ = kcp.write_all(b"hello world").await;
                                    log::debug!("{:?}", String::from_utf8_lossy(&buf));
                                }
                            });
                        }
                        Err(_) => {}
                    }
                }
            });
    }

    #[test]
    pub fn test_kcp_client() {
        init_logger();

        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let udp = tokio::net::UdpSocket::bind("0.0.0.0:0").await.unwrap();
                udp.connect("127.0.0.1:7777").await.unwrap();

                let kcp = KcpConnector::new(Arc::new(udp));

                let mut kcp = kcp.connect().await.unwrap();

                loop {  
                   
                    let mut buf = String::new();
                    std::io::stdin().read_line(&mut buf).unwrap();
                    let _ = kcp.write_all(buf.as_bytes()).await;
                    let mut buf = Vec::new();
                    buf.resize(1500, 0);
                    let n = kcp.read(&mut buf).await.unwrap();
                    buf.truncate(n);
                    log::debug!("{:?}", buf)
                }

            });
    }


}
