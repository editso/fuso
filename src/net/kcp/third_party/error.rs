//! KCP https://github.com/Matrix-Zhang/kcp

use std::error::Error as StdError;
use std::fmt;
use std::io::{self, ErrorKind};

/// KCP protocol errors
#[derive(Debug)]
pub enum Error {
    ConvInconsistent(u32, u32),
    InvalidMtu(usize),
    InvalidSegmentSize(usize),
    InvalidSegmentDataSize(usize, usize),
    IoError(io::Error),
    NeedUpdate,
    RecvQueueEmpty,
    ExpectingFragment,
    UnsupportedCmd(u8),
    UserBufTooBig,
    UserBufTooSmall,
}

impl StdError for Error {
    fn cause(&self) -> Option<&dyn StdError> {
        match *self {
            Error::IoError(ref e) => Some(e),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match *self {
            Error::ConvInconsistent(ref s, ref o) => {
                write!(f, "conv inconsistent, expected {}, found {}", *s, *o)
            }
            Error::InvalidMtu(ref e) => write!(f, "invalid mtu {}", *e),
            Error::InvalidSegmentSize(ref e) => write!(f, "invalid segment size of {}", *e),
            Error::InvalidSegmentDataSize(ref s, ref o) => {
                write!(
                    f,
                    "invalid segment data size, expected {}, found {}",
                    *s, *o
                )
            }
            Error::IoError(ref e) => e.fmt(f),
            Error::UnsupportedCmd(ref e) => write!(f, "cmd {} is not supported", *e),
            ref e => write!(f, "{}", e),
        }
    }
}

fn make_io_error<T>(kind: ErrorKind, msg: T) -> io::Error
where
    T: Into<Box<dyn StdError + Send + Sync>>,
{
    io::Error::new(kind, msg)
}

impl From<Error> for io::Error {
    fn from(err: Error) -> io::Error {
        let kind = match err {
            Error::ConvInconsistent(..) => ErrorKind::Other,
            Error::InvalidMtu(..) => ErrorKind::Other,
            Error::InvalidSegmentSize(..) => ErrorKind::Other,
            Error::InvalidSegmentDataSize(..) => ErrorKind::Other,
            Error::IoError(err) => return err,
            Error::NeedUpdate => ErrorKind::Other,
            Error::RecvQueueEmpty => ErrorKind::WouldBlock,
            Error::ExpectingFragment => ErrorKind::WouldBlock,
            Error::UnsupportedCmd(..) => ErrorKind::Other,
            Error::UserBufTooBig => ErrorKind::Other,
            Error::UserBufTooSmall => ErrorKind::Other,
        };

        make_io_error(kind, err)
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::IoError(err)
    }
}
