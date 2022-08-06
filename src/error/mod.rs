use std::fmt::Display;

use crate::{kcp, Socket, SocketKind};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct Error {
    kind: Kind,
}

#[derive(Debug)]
pub enum InvalidAddr {
    Domain(String),
    Socket(std::net::AddrParseError),
}

#[derive(Debug)]
pub enum SyncErr {
    Mutex,
}

#[derive(Debug)]
pub enum Encoding {
    FromUtf8(std::string::FromUtf8Error),
}

#[derive(Debug)]
pub enum PacketErr {
    Head([u8; 4]),
}

#[derive(Debug)]
pub enum SocksErr {
    Protocol,
    Authenticate,
    InvalidAddress,
    BindNotSupport,
    Socks5Frg,
    Head { ver: u8, nmethod: u8 },
    Method(u8),
    BadLength { expect: usize, current: usize },
}

#[derive(Debug)]
pub enum Lz4Err {
    Compress,
    Decompress,
    DecodeReleased,
    EncodeReleased,
}

#[derive(Debug)]
pub enum CompressErr {
    Lz4(Lz4Err),
}

#[derive(Debug)]
pub enum SocketErr {
    BadAddress(Socket),
    NotSupport(Socket),
    NotExpected { expect: SocketKind, current: Socket },
}

#[derive(Debug)]
pub enum AesErr {
    Pad(cbc::cipher::inout::PadError),
    UnPad(cbc::cipher::inout::block_padding::UnpadError),
    NotEqual(cbc::cipher::inout::NotEqualError),
    InvalidLength(cbc::cipher::InvalidLength),
}

#[derive(Debug)]
pub enum EncryptionErr {
    Aes(AesErr),
    Rsa(rsa::errors::Error),
    RsaPkcs7(rsa::pkcs1::Error),
    RsaSpki(rsa::pkcs8::spki::Error),
}

#[derive(Debug)]
pub enum MixErr {
    Unsupported(Socket),
}

#[derive(Debug)]
pub enum Kind {
    Channel,
    AlreadyUsed,
    IO(std::io::Error),
    #[cfg(feature = "fuso-rt-tokio")]
    Timeout(tokio::time::error::Elapsed),
    #[cfg(feature = "fuso-rt-smol")]
    Timeout(std::time::Instant),
    Memory,
    Mark,
    Sync(SyncErr),
    Deserialize(String),
    InvalidAddr(InvalidAddr),
    Encoding(Encoding),
    Packet(PacketErr),
    Fallback,
    Unexpected(String),
    Message(String),
    Socks(SocksErr),
    Once,
    BadForward,
    Kcp(kcp::KcpErr),
    Compress(CompressErr),
    Socket(SocketErr),
    Encryption(EncryptionErr),
    Mix(MixErr),
    Unsupported(Socket),
    AddressLoop(Socket),
    Improper(Socket),
    Text(String),
    MaxRetries(usize),
}

impl Display for SyncErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", {
            match self {
                SyncErr::Mutex => "mutex",
            }
        })
    }
}

impl Display for InvalidAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", {
            match self {
                InvalidAddr::Domain(domain) => format!("invalid domain {}", domain),
                InvalidAddr::Socket(addr) => format!("parse addr {}", addr),
            }
        })
    }
}

impl Display for Encoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", {
            match self {
                Encoding::FromUtf8(utf8) => format!("encoding utf8 {}", utf8),
            }
        })
    }
}

impl Display for PacketErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", {
            match self {
                PacketErr::Head(e) => format!("invalid packet head {:?}", e),
            }
        })
    }
}

impl Display for SocksErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", {
            match self {
                SocksErr::Protocol => format!("invalid socks5 protocol"),
                SocksErr::Authenticate => format!("socks5 authentication failed"),
                SocksErr::InvalidAddress => format!("invalid address"),
                SocksErr::BindNotSupport => format!("bind not support"),
                SocksErr::Socks5Frg => format!("fragment not support"),
                SocksErr::Head { ver, nmethod } => {
                    format!("invalid socks5 head ver={}, nmethod={}", ver, nmethod)
                }
                SocksErr::Method(e) => format!("method err {}", e),
                SocksErr::BadLength { expect, current } => {
                    format!("bad socks5 packet expect {} , current={}", expect, current)
                }
            }
        })
    }
}

impl From<cbc::cipher::inout::NotEqualError> for Error {
    fn from(e: cbc::cipher::inout::NotEqualError) -> Self {
        Kind::Encryption(EncryptionErr::Aes(AesErr::NotEqual(e))).into()
    }
}

impl From<cbc::cipher::inout::PadError> for Error {
    fn from(e: cbc::cipher::inout::PadError) -> Self {
        Kind::Encryption(EncryptionErr::Aes(AesErr::Pad(e))).into()
    }
}

impl From<cbc::cipher::InvalidLength> for Error {
    fn from(e: cbc::cipher::InvalidLength) -> Self {
        Kind::Encryption(EncryptionErr::Aes(AesErr::InvalidLength(e))).into()
    }
}

impl From<cbc::cipher::block_padding::UnpadError> for Error {
    fn from(e: cbc::cipher::block_padding::UnpadError) -> Self {
        Kind::Encryption(EncryptionErr::Aes(AesErr::UnPad(e))).into()
    }
}

impl From<rsa::pkcs1::Error> for Error {
    fn from(e: rsa::pkcs1::Error) -> Self {
        Kind::Encryption(EncryptionErr::RsaPkcs7(e)).into()
    }
}

impl From<rsa::pkcs8::spki::Error> for Error {
    fn from(e: rsa::pkcs8::spki::Error) -> Self {
        Kind::Encryption(EncryptionErr::RsaSpki(e)).into()
    }
}

impl Display for SocketErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let fmt = match self {
            SocketErr::BadAddress(socket) => format!("invalid socket {}", socket),
            Self::NotSupport(socket) => format!("socket not support {}", socket),
            SocketErr::NotExpected { expect, current } => format!(
                "socket not expected ! expect: {}, current: {}, addr: {}",
                expect,
                current,
                current.addr()
            ),
        };

        writeln!(f, "{}", fmt)
    }
}

impl Display for AesErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", {
            match self {
                AesErr::Pad(e) => format!("{}", e),
                AesErr::UnPad(e) => format!("{}", e),
                AesErr::NotEqual(e) => format!("{}", e),
                AesErr::InvalidLength(e) => format!("{}", e),
            }
        })
    }
}

impl Display for EncryptionErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", {
            match self {
                EncryptionErr::Aes(e) => format!("{}", e),
                EncryptionErr::Rsa(e) => format!("{}", e),
                EncryptionErr::RsaPkcs7(e) => format!("{}", e),
                EncryptionErr::RsaSpki(e) => format!("{}", e),
            }
        })
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let fmt = match self.kind() {
            Kind::Channel => format!("Channel"),
            Kind::AlreadyUsed => format!("AlreadyUsed"),
            Kind::IO(io) => format!("{}", io),
            Kind::Timeout(timeout) => format!("{}", timeout),
            Kind::Memory => format!(""),
            Kind::Mark => format!("mark"),
            Kind::Sync(e) => format!("{}", e),
            Kind::Deserialize(e) => format!("{}", e),
            Kind::InvalidAddr(e) => format!("{}", e),
            Kind::Encoding(e) => format!("{}", e),
            Kind::Packet(e) => format!("{}", e),
            Kind::Fallback => format!("fallback"),
            Kind::Unexpected(e) => format!("{}", e),
            Kind::Message(message) => format!("{}", message),
            Kind::Socks(e) => format!("{}", e),
            Kind::Once => format!("call once"),
            Kind::BadForward => format!("bad forward"),
            Kind::Kcp(e) => format!("{}", e),
            Kind::Compress(e) => {
                format!("{:?}", e)
            }
            Kind::Socket(socket) => format!("{}", socket),
            Kind::Encryption(e) => format!("{}", e),
            Kind::Mix(e) => format!("{:?}", e),
            Kind::Unsupported(e) => format!("Unsupported {}", e),
            Kind::AddressLoop(e) => format!("address loop {}", e),
            Kind::Improper(e) => format!("no suitable ones {}", e),
            Kind::Text(txt) => format!("{}", txt),
            Kind::MaxRetries(retry) => format!("exceeded maximum number of attempts {}", retry),
        };
        write!(f, "{}", fmt)
    }
}

impl From<InvalidAddr> for Error {
    fn from(addr: InvalidAddr) -> Self {
        Self {
            kind: Kind::InvalidAddr(addr),
        }
    }
}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Kind::Text(s).into()
    }
}

impl From<MixErr> for Error {
    fn from(e: MixErr) -> Self {
        Kind::Mix(e).into()
    }
}

impl From<rsa::errors::Error> for Error {
    fn from(e: rsa::errors::Error) -> Self {
        Kind::Encryption(EncryptionErr::Rsa(e)).into()
    }
}

impl From<SyncErr> for Error {
    fn from(e: SyncErr) -> Self {
        Self {
            kind: Kind::Sync(e),
        }
    }
}

impl From<SocksErr> for Error {
    fn from(e: SocksErr) -> Self {
        Self {
            kind: Kind::Socks(e),
        }
    }
}

impl From<Kind> for Error {
    fn from(kind: Kind) -> Self {
        Self { kind }
    }
}

impl From<SocketErr> for Error {
    fn from(socket: SocketErr) -> Self {
        Self {
            kind: Kind::Socket(socket),
        }
    }
}

impl From<std::io::ErrorKind> for Error {
    fn from(e: std::io::ErrorKind) -> Self {
        Kind::IO(e.into()).into()
    }
}

impl From<Encoding> for Error {
    fn from(e: Encoding) -> Self {
        Kind::Encoding(e).into()
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(e: std::string::FromUtf8Error) -> Self {
        Encoding::FromUtf8(e).into()
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Kind::IO(e).into()
    }
}

impl From<kcp::KcpErr> for Error {
    fn from(e: kcp::KcpErr) -> Self {
        Kind::Kcp(e).into()
    }
}

impl<T> From<std::sync::PoisonError<T>> for Error {
    fn from(_: std::sync::PoisonError<T>) -> Self {
        SyncErr::Mutex.into()
    }
}

impl From<std::cell::BorrowMutError> for Error {
    fn from(_: std::cell::BorrowMutError) -> Self {
        SyncErr::Mutex.into()
    }
}

impl From<bincode::Error> for Error {
    fn from(e: bincode::Error) -> Self {
        Kind::Deserialize(e.to_string()).into()
    }
}

#[cfg(feature = "fuso-rt-tokio")]
impl From<tokio::time::error::Elapsed> for Error {
    fn from(e: tokio::time::error::Elapsed) -> Self {
        Kind::Timeout(e).into()
    }
}

impl From<async_channel::RecvError> for Error {
    fn from(_: async_channel::RecvError) -> Self {
        Kind::Channel.into()
    }
}

impl<T> From<async_channel::SendError<T>> for Error {
    fn from(_: async_channel::SendError<T>) -> Self {
        Kind::Channel.into()
    }
}

impl From<PacketErr> for Error {
    fn from(e: PacketErr) -> Self {
        Kind::Packet(e).into()
    }
}

#[cfg(feature = "fuso-rt-smol")]
impl From<std::time::Instant> for Error {
    fn from(e: std::time::Instant) -> Self {
        Kind::Timeout(e).into()
    }
}

impl From<Lz4Err> for Error {
    fn from(e: Lz4Err) -> Self {
        Kind::Compress(CompressErr::Lz4(e)).into()
    }
}

impl std::error::Error for Error {}

impl Error {
    pub fn kind(&self) -> &Kind {
        &self.kind
    }

    pub fn is_packet_err(&self) -> bool {
        match &self.kind {
            Kind::Packet(_) => true,
            _ => false,
        }
    }

    pub fn is_socks_error(&self) -> bool {
        match &self.kind {
            Kind::Socks(SocksErr::Head { ver: _, nmethod: _ }) => true,
            _ => false,
        }
    }

    pub fn is_not_support(&self) -> bool {
        match &self.kind {
            Kind::Unsupported(_) => true,
            _ => false,
        }
    }
}
