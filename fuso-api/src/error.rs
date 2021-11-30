use std::fmt::Display;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct Error {
    repr: Repr,
}

#[derive(Debug)]
pub enum ErrorKind {
    BadPacket,
    
}

#[derive(Debug)]
pub enum Repr {
    Fuso(ErrorKind),
    IO(std::io::Error),
}

impl Error {
    #[inline]
    pub fn new(kind: ErrorKind) -> Self {
        kind.into()
    }

    #[inline]
    pub fn with_io(err: std::io::Error)->Self{
        err.into()
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.repr, f)
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self {
            repr: Repr::IO(error),
        }
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Self {
            repr: Repr::Fuso(kind),
        }
    }
}

impl From<std::io::ErrorKind> for Error {
    fn from(kind: std::io::ErrorKind) -> Self {
        Self {
            repr: Repr::IO(kind.into()),
        }
    }
}
