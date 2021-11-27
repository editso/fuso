use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

pub fn parse_addr(host: &str, port: &str) -> std::result::Result<SocketAddr, String> {
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
    )
}
