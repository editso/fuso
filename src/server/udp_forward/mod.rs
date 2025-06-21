use std::net::SocketAddr;

use crate::{
    core::{
        future::Select,
        io::AsyncWriteExt,
        net::{AsyncRecvFromExt, AsyncSendToExt, UdpSocket},
        protocol::{AsyncPacketRead, AsyncPacketSend},
        rpc::{Decoder, Encoder},
        stream::{
            fragment::Fragment,
            virio::{self, Vitio},
            Socks5UdpStream,
        },
        transfer::TransmitterExt,
        AbstractStream, BoxedFuture,
    },
    error, unpack_socks_addr,
};

type Connection = crate::core::Connection<'static>;

pub trait UdpForwarder {
    fn open<'a>(&'a self, input: Connection) -> BoxedFuture<'a, error::Result<Connection>>;
}

pub struct UdpForwarderImpl {
    vio: Vitio,
    addr: SocketAddr,
}

impl UdpForwarderImpl {
    pub fn new(addr: SocketAddr, udp: UdpSocket) -> (Select<error::Result<()>>, Self) {
        let mut poller = Select::new();
        let (vio_1, vio_2) = virio::open();

        let (mut writer, mut reader) = vio_2.split();

        poller.add({
            let mut udp = udp.clone();
            async move {
                log::debug!("socks5 udp forward bind at: {addr}");

                let mut buf = [0u8; 1400];

                loop {
                    let (addr, n) = udp.recvfrom(&mut buf).await?;

                    if let Ok(pkt) = fuso_socks::ForwardPacket::unpack(&buf[..n]).await {
                        log::trace!(
                            "receive udp forward packet({}bytes) from {} to {}",
                            pkt.data().len(),
                            addr,
                            pkt.addr()
                        );
                        
                        let (mut daddr, port) = unpack_socks_addr!(pkt.addr);

                        writer
                            .send_packet(&{
                                Fragment::UdpForward {
                                    saddr: addr,
                                    daddr: unsafe { daddr.0.pop().unwrap_unchecked() },
                                    dport: port,
                                    data: pkt.data,
                                }
                                .encode()?
                            })
                            .await?;
                    }
                }
            }
        });

        poller.add(async move {
            let mut udp = udp;
            loop {
                let pkt = reader.recv_packet().await?;
                let cloned = pkt.clone();

        
                let fragment: error::Result<Fragment> = cloned.decode();

                match fragment {
                    Ok(frg) => {
                        match frg {
                            Fragment::Ping => todo!(),
                            Fragment::UdpForward { .. } => todo!(),
                            Fragment::Udp(dst, data) => {
                                log::trace!("response to {} -> {:?}", dst, data);
        
                                let packed = fuso_socks::ForwardPacket::pack(addr, &data);
        
                                udp.send_to(&dst, &packed).await?;
                            }
                        }
                    },
                    Err(e) => {
                        log::error!("{e:?}: {pkt:?}", )
                    },
                }

                
            }
        });

        (poller, Self { vio: vio_1, addr })
    }
}

impl UdpForwarder for UdpForwarderImpl {
    fn open<'a>(&'a self, mut input: Connection) -> BoxedFuture<'a, error::Result<Connection>> {
        Box::pin(async move {
            let mut buf = Vec::new();

            buf.extend(&[0x05, 0x00, 0x00]);

            match self.addr {
                std::net::SocketAddr::V4(addr) => {
                    buf.push(0x01);
                    buf.extend(addr.ip().octets());
                    buf.extend(addr.port().to_be_bytes());
                }
                std::net::SocketAddr::V6(_) => unimplemented!(),
            }

            input.write_all(&buf).await?;

            let addr = input.addr().clone();

            let stream = Socks5UdpStream::new(
                AbstractStream::new(input),
                AbstractStream::new(self.vio.clone()),
            );

            Ok(Connection::new(addr, AbstractStream::new(stream)))
        })
    }
}

impl UdpForwarder for () {
    fn open<'a>(&'a self, mut input: Connection) -> BoxedFuture<'a, error::Result<Connection>> {
        Box::pin(async move {
            input
                .write_all(&[0x05, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
                .await?;

            Err(error::FusoError::Cancel)
        })
    }
}
