use std::fmt::Display;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct Error {
    repr: Repr,
}

#[derive(Debug)]
pub enum ErrorKind {
    BadPacket,
    UnHandler,
    Customer(String),
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
    pub fn with_io(err: std::io::Error) -> Self {
        err.into()
    }
}

impl Display for Error {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.repr, f)
    }
}

impl From<std::io::Error> for Error {
    #[inline]
    fn from(error: std::io::Error) -> Self {
        Self {
            repr: Repr::IO(error),
        }
    }
}

impl From<ErrorKind> for Error {
    #[inline]
    fn from(kind: ErrorKind) -> Self {
        Self {
            repr: Repr::Fuso(kind),
        }
    }
}

impl From<std::io::ErrorKind> for Error {
    #[inline]
    fn from(kind: std::io::ErrorKind) -> Self {
        Self {
            repr: Repr::IO(kind.into()),
        }
    }
}

impl From<smol::channel::RecvError> for Error {
    #[inline]
    fn from(e: smol::channel::RecvError) -> Self {
        Self {
            repr: Repr::IO(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )),
        }
    }
}

impl<T> From<smol::channel::SendError<T>> for Error
where
    T: Into<String>,
{
    #[inline]
    fn from(e: smol::channel::SendError<T>) -> Self {
        Self {
            repr: Repr::IO(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )),
        }
    }
}

impl From<&str> for Error {
    #[inline]
    fn from(txt: &str) -> Self {
        Self {
            repr: Repr::Fuso(ErrorKind::Customer(txt.into())),
        }
    }
}

impl From<String> for Error {
    #[inline]
    fn from(txt: String) -> Self {
        Self {
            repr: Repr::Fuso(ErrorKind::Customer(txt.into())),
        }
    }
}
