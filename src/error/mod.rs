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
pub enum Encoding {
    FromUtf8(std::string::FromUtf8Error),
}

#[derive(Debug)]
pub enum Kind {
    IO(std::io::Error),
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

impl std::error::Error for Error {}
