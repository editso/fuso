use std::{pin::Pin, task::Context};

use crate::error::Result;

pub trait Register {
    type Output;
    type Metadata;
    fn poll_register(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        metadata: &Self::Metadata,
    ) -> Result<Self::Output>;
}

pub trait Encrypt {
    fn poll_encrypt_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Result<Vec<u8>>;
}

pub trait Decrypt {
    fn poll_decrypt_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Result<()>;
}
