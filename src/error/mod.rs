use std::fmt::Display;

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
pub enum Sync {
    Mutex,
}

#[derive(Debug)]
pub enum Encoding {
    FromUtf8(std::string::FromUtf8Error),
}

#[derive(Debug)]
pub enum Kind {
    IO(std::io::Error),
    #[cfg(feature = "fuso-rt-tokio")]
    Timeout(tokio::time::error::Elapsed),
    #[cfg(feature = "fuso-rt-smol")]
    Timeout(std::time::Instant),
    Memory,
    Sync(Sync),
    Deserialize(String),
    InvalidAddr(InvalidAddr),
    Encoding(Encoding),
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<InvalidAddr> for Error {
    fn from(addr: InvalidAddr) -> Self {
        Self {
            kind: Kind::InvalidAddr(addr),
        }
    }
}

impl From<Sync> for Error {
    fn from(e: Sync) -> Self {
        Self {
            kind: Kind::Sync(e),
        }
    }
}

impl From<Kind> for Error {
    fn from(kind: Kind) -> Self {
        Self { kind }
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

impl<T> From<std::sync::PoisonError<T>> for Error {
    fn from(_: std::sync::PoisonError<T>) -> Self {
        Sync::Mutex.into()
    }
}

impl From<std::cell::BorrowMutError> for Error {
    fn from(_: std::cell::BorrowMutError) -> Self {
        Sync::Mutex.into()
    }
}

#[cfg(feature = "fuso-rt-tokio")]
impl From<tokio::time::error::Elapsed> for Error {
    fn from(e: tokio::time::error::Elapsed) -> Self {
        Kind::Timeout(e).into()
    }
}

#[cfg(feature = "fuso-rt-smol")]
impl From<std::time::Instant> for Error{
    fn from(e: std::time::Instant) -> Self {
        Kind::Timeout(e).into()
    }
}

impl std::error::Error for Error {}
