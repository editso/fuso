pub type Result<T> = std::result::Result<T, FusoError>;


#[derive(Debug)]
pub enum FusoError{
  BadMagic,
  Timeout,
  Abort,
  BadRpcCall(u64),
  TomlDeError(toml::de::Error),
  StdIo(std::io::Error),
}



impl From<std::io::Error> for FusoError{
  fn from(value: std::io::Error) -> Self {
      Self::StdIo(value)
  }
}

impl From<toml::de::Error> for FusoError{
  fn from(value: toml::de::Error) -> Self {
      Self::TomlDeError(value)
  }
}