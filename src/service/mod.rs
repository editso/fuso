use std::net::IpAddr;

pub struct PortForward{
  bind: IpAddr,
  ports: Vec<u16>,
}


