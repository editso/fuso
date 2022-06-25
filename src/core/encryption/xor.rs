use crate::{AsyncRead, AsyncWrite};

use super::Encryption;

pub struct Cipher<S> {
    stream: S,
}

