#[cfg(feature = "fuso-rt-smol")]
pub use smol::lock::Mutex;

#[cfg(feature = "fuso-rt-tokio")]
pub use tokio::sync::Mutex;
