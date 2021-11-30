pub mod client;
pub mod cmd;
pub mod server;
pub mod core;
pub mod retain;
pub mod ciphe;
pub mod buffer;

use std::sync::Arc;

pub use fuso_api::*;
use futures::{AsyncRead, AsyncReadExt, AsyncWrite};
use smol::lock::Mutex;




#[inline]
pub fn split<T>(o: T) -> (T, T)
where
    T: Clone,
{
    (o.clone(), o)
}

#[inline]
pub fn split_mutex<T>(o: T) -> (Arc<Mutex<T>>, Arc<Mutex<T>>) {
    let mutex = Arc::new(Mutex::new(o));
    split(mutex)
}

pub async fn forward<I, O>(origin: I, target: O) -> std::io::Result<()>
where
    I: AsyncWrite + AsyncRead + Unpin + Send + Sync + 'static,
    O: AsyncWrite + AsyncRead + Unpin + Send + Sync + 'static,
{
    let (reader, writer) = origin.split();
    let (reader_t, write_t) = target.split();

    smol::future::race(
        smol::io::copy(reader, write_t),
        smol::io::copy(reader_t, writer),
    )
    .await?;

    Ok(())
}
