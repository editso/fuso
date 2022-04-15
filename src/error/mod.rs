use std::fmt::Display;

pub type Result<T> = std::result::Result<T, Error>;


#[derive(Debug)]
pub struct Error{
    kind: Kind
}

#[derive(Debug)]
pub enum Kind{
    IO(std::io::Error)
}

impl Display for Error{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for Error{
    
}