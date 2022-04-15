#[cfg(not(feature = "fuso-rt-tokio"))]
pub use futures::{AsyncRead, AsyncWrite};

#[cfg(feature = "fuso-rt-tokio")]
pub use tokio::io::{AsyncRead, AsyncWrite};
