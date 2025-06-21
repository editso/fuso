use std::fmt::Display;

pub type Result<T> = std::result::Result<T, FusoError>;

#[derive(Debug)]
pub enum FusoError {
    BadMagic,
    Timeout,
    Abort,
    AuthError,
    Cancel,
    UdpForwardTerm,
    InvalidConnection,
    InvaledSetter,
    BadRpcCall(u64),
    InvalidPort,
    InvalidExposeType,
    NotResponse,
    Custom(String),
    UnsupportProtocol,
    KcpError(kcp_rust::KcpError),
    Utf8Error(std::str::Utf8Error),
    BadCString(std::ffi::NulError),
    MsgPack(MsgPack),
    TomlDeError(toml::de::Error),
    StdIo(std::io::Error),
}

#[derive(Debug)]
pub enum MsgPack {
    Encode(rmp_serde::encode::Error),
    Decode(rmp_serde::decode::Error),
}

impl std::error::Error for FusoError {}

impl From<std::io::Error> for FusoError {
    fn from(value: std::io::Error) -> Self {
        Self::StdIo(value)
    }
}

impl From<toml::de::Error> for FusoError {
    fn from(value: toml::de::Error) -> Self {
        Self::TomlDeError(value)
    }
}

impl From<std::ffi::NulError> for FusoError{
    fn from(value: std::ffi::NulError) -> Self {
        Self::BadCString(value)
    }
}

impl From<rmp_serde::encode::Error> for FusoError {
    fn from(value: rmp_serde::encode::Error) -> Self {
        Self::MsgPack(MsgPack::Encode(value))
    }
}

impl From<kcp_rust::KcpError> for FusoError{
    fn from(value: kcp_rust::KcpError) -> Self {
        Self::KcpError(value)
    }
}

impl From<rmp_serde::decode::Error> for FusoError {
    fn from(value: rmp_serde::decode::Error) -> Self {
        Self::MsgPack(MsgPack::Decode(value))
    }
}

impl From<String> for FusoError {
    fn from(value: String) -> Self {
        Self::Custom(value)
    }
}

impl From<FusoError> for std::io::Error {
    fn from(value: FusoError) -> Self {
        match value {
            FusoError::StdIo(io) => io,
            value => std::io::Error::new(std::io::ErrorKind::Other, value),
        }
    }
}

impl From<std::str::Utf8Error> for FusoError{
    fn from(value: std::str::Utf8Error) -> Self {
        Self::Utf8Error(value)
    }
}

impl Display for FusoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
