#[cfg(feature = "fuso-rt-fuso")]
mod smol;

#[cfg(feature = "fuso-rt-tokio")]
mod tokio;
