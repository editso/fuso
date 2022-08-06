mod lz4;
pub use lz4::Lz4Compress;

use std::{
    pin::Pin,
    task::{Context, Poll},
};

pub trait Encoder {
    fn poll_encode_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<crate::Result<usize>>;
}

pub trait Decoder {
    fn poll_decode_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut crate::ReadBuf<'_>,
    ) -> Poll<crate::Result<usize>>;
}
