mod buffer;
mod core;
mod error;
mod stream;
mod udp;

pub use crate::buffer::*;
pub use crate::core::*;
pub use crate::error::*;
pub use crate::stream::*;
pub use crate::udp::*;
pub use async_trait::*;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures::AsyncReadExt;
    use smol::{future::FutureExt, io::AsyncWriteExt, net::UdpSocket};

    use crate::{core::Packet, now_mills, UdpListener, UdpStreamEx};

    fn init_logger() {
        env_logger::builder()
            .filter_level(log::LevelFilter::Debug)
            .init();
    }

    #[test]
    fn test_time() {
        // let time = std::time::SystemTime::now()
        // .duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap();

        println!("{:?}", now_mills())
    }

    #[test]
    fn test_packet() {
        init_logger();

        let packet = Packet::new(1, "Hello".into());

        log::debug!("{:?}", Packet::size());
        log::debug!("{:?}", packet);

        assert_eq!(5, packet.get_len());

        let data = packet.encode();

        log::debug!("len: {}, raw: {:?}", data.len(), data);

        let packet = Packet::decode_data(&data).unwrap();

        log::debug!("data len: {}, {:?}", packet.get_len(), packet.get_data());
    }

    #[test]
    fn test_udp_stream() {
        init_logger();

        smol::block_on(async move {
            let listen_future = async move {
                match UdpListener::bind("0.0.0.0:9999").await {
                    Ok(udp) => match udp.accept().await {
                        Ok(mut stream) => {
                            log::debug!("new udp");
                            let mut buf = Vec::new();
                            buf.resize(1024, 0);
                            match stream.read(&mut buf).await {
                                Ok(n) => {
                                    buf.truncate(n);
                                    log::debug!("recv {}", String::from_utf8_lossy(&buf));
                                    stream.write_all(b"Hello!").await.unwrap();
                                    smol::Timer::after(Duration::from_millis(2000)).await;
                                }
                                Err(e) => {
                                    log::error!("recv {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("accept {}", e);
                        }
                    },
                    Err(e) => {
                        log::error!("bind {}", e);
                    }
                };
            };

            let client_future = async move {
                smol::Timer::after(Duration::from_millis(2000)).await;

                match UdpSocket::bind("0.0.0.0:0").await {
                    Ok(udp) => {
                        udp.connect("127.0.0.1:9999").await.unwrap();
                        let mut udp = udp.as_udp_stream();

                        udp.write(b"Hello World").await.unwrap();
                        let mut buf = Vec::new();
                        buf.resize(1024, 0);

                        match udp.read(&mut buf).await {
                            Ok(n) => {
                                buf.truncate(n);
                                log::debug!("read {}", String::from_utf8_lossy(&buf));
                            }
                            Err(e) => {
                                log::error!("read {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("connect {}", e);
                    }
                }
            };

            client_future.race(listen_future).await
        });
    }
}
