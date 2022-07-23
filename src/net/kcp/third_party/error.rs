//! KCP https://github.com/Matrix-Zhang/kcp

use std::error::Error as StdError;
use std::fmt;
use std::io::{self, ErrorKind};

/// KCP protocol errors
#[derive(Debug)]
pub enum KcpErr {
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
    NoMoreConv,
    ConnectionReset,
}

impl StdError for KcpErr {
    fn cause(&self) -> Option<&dyn StdError> {
        match *self {
            KcpErr::IoError(ref e) => Some(e),
            _ => None,
        }
    }
}

impl fmt::Display for KcpErr {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match *self {
            KcpErr::ConvInconsistent(ref s, ref o) => {
                write!(f, "conv inconsistent, expected {}, found {}", *s, *o)
            }
            KcpErr::InvalidMtu(ref e) => write!(f, "invalid mtu {}", *e),
            KcpErr::InvalidSegmentSize(ref e) => write!(f, "invalid segment size of {}", *e),
            KcpErr::InvalidSegmentDataSize(ref s, ref o) => {
                write!(
                    f,
                    "invalid segment data size, expected {}, found {}",
                    *s, *o
                )
            }
            KcpErr::IoError(ref e) => e.fmt(f),
            KcpErr::UnsupportedCmd(ref e) => write!(f, "cmd {} is not supported", *e),
            KcpErr::NeedUpdate => write!(f, "NeedUpdate"),
            KcpErr::RecvQueueEmpty => write!(f, "RecvQueueEmpty"),
            KcpErr::ExpectingFragment => write!(f, "ExpectingFragment"),
            KcpErr::UserBufTooBig => write!(f, "UserBufTooBig"),
            KcpErr::UserBufTooSmall => write!(f, "UserBufTooSmall"),
            KcpErr::NoMoreConv => write!(f, "NoMoreConv"),
            KcpErr::ConnectionReset => write!(f, "ConnectionReset"),
        }
    }
}

fn make_io_error<T>(kind: ErrorKind, msg: T) -> io::Error
where
    T: Into<Box<dyn StdError + Send + Sync>>,
{
    io::Error::new(kind, msg)
}

impl From<KcpErr> for io::Error {
    fn from(err: KcpErr) -> io::Error {
        let kind = match err {
            KcpErr::ConvInconsistent(..) => ErrorKind::Other,
            KcpErr::InvalidMtu(..) => ErrorKind::Other,
            KcpErr::InvalidSegmentSize(..) => ErrorKind::Other,
            KcpErr::InvalidSegmentDataSize(..) => ErrorKind::Other,
            KcpErr::IoError(err) => return err,
            KcpErr::NeedUpdate => ErrorKind::Other,
            KcpErr::RecvQueueEmpty => ErrorKind::WouldBlock,
            KcpErr::ExpectingFragment => ErrorKind::WouldBlock,
            KcpErr::UnsupportedCmd(..) => ErrorKind::Other,
            KcpErr::UserBufTooBig => ErrorKind::Other,
            KcpErr::UserBufTooSmall => ErrorKind::Other,
            KcpErr::NoMoreConv => ErrorKind::Other,
            KcpErr::ConnectionReset => ErrorKind::ConnectionReset,
        };

        make_io_error(kind, err)
    }
}

impl From<io::Error> for KcpErr {
    fn from(err: io::Error) -> KcpErr {
        KcpErr::IoError(err)
    }
}
