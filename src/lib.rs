pub mod crypt;

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

pub use crypt::*;

pub fn parse_addr(host: &str, port: &str) -> std::result::Result<String, String> {
    host.parse::<IpAddr>().map_or(
        format!("{}:{}", host, port).to_socket_addrs().map_or(
            Err(String::from("Invalid host")),
            |mut addrs| {
                addrs
                    .next()
                    .map_or(Err(String::from("Invalid host")), |addr| Ok(addr))
            },
        ),
        |ip| {
            port.parse::<u16>()
                .map_or(Err(String::from("Invalid port number")), |port| {
                    Ok(SocketAddr::new(ip, port))
                })
        },
    )?;

    Ok(format!("{}:{}", host, port))
}

#[cfg(test)]
mod tests {
    
    use futures::{

        AsyncReadExt, AsyncWriteExt,
    };

    use smol::{
        net::{TcpListener, TcpStream},
    };

    #[test]
    fn test_clap() {
        use clap::load_yaml;
        use clap::App;

        let yaml = load_yaml!("assets/client-cfg.yml");

        
        let _ = App::from(yaml).get_matches();
    }

    fn init_logger() {
        env_logger::builder()
            .filter_level(log::LevelFilter::Debug)
            .init();
    }

    #[test]
    fn test_websocket_server() {
        init_logger();

        let future = async move {
            let tcp = TcpListener::bind("0.0.0.0:9003")
                .await
                .expect("address used");

            loop {
                let (mut stream, _) = tcp.accept().await.unwrap();

                smol::spawn(async move {
                    let mut buf = Vec::new();
                    buf.resize(1024, 0);
                    let n = stream.read(&mut buf).await.unwrap();
                    buf.truncate(n);

                    log::debug!("{}", String::from_utf8_lossy(&buf));

                    stream.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n")
                        .await
                        .unwrap();
                
                    loop {
                        let mut buf = Vec::new();
                        buf.resize(1024, 0);
                        
                        let n = stream.read(&mut buf).await.unwrap();
                        buf.truncate(n);

                        if n == 0{
                            break;
                        }

                        log::debug!("recv {:?}", String::from_utf8_lossy(&buf));

                        stream.write_all(&buf).await.expect("response msg error");

                    }
                })
                .detach();
            }
        };

        smol::block_on(future);
    }

    #[test]
    fn test_websocket_client() {
        init_logger();
        let future = async move {
            let args: Vec<String> = std::env::args().collect();

            let addr = args.get(2).expect("require addr");

            let mut stream = TcpStream::connect(addr).await.expect("connect failure");

            stream
                .write_all(format!("GET / HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nbConnection: Upgrade\r\n\r\n", addr).as_bytes())
                .await
                .expect("write header error");

            let mut buf = Vec::new();
            buf.resize(1024, 0);

            let n = stream.read(&mut buf).await.expect("read header error");

            buf.truncate(n);

            log::debug!("recv = {:?}", String::from_utf8_lossy(&buf));

            loop {
                stream
                    .write_all(b"hello world")
                    .await
                    .expect("write msg failure");

                buf.resize(1024, 0);

                let n = stream.read(&mut buf).await.expect("read header error");

                buf.truncate(n);

                if n == 0 {
                    break;
                }

                log::debug!("recv = {:?}", String::from_utf8_lossy(&buf));
            }
        };

        smol::block_on(future);
    }
}
