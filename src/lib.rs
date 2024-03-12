#[cfg(feature = "fuso-cli")]
pub mod cli;

pub mod core;

#[cfg(feature = "fuso-config")]
pub mod config;

pub mod service;

pub mod server;

pub mod client;

pub mod error;

pub mod runtime;


pub fn enter_async_main<F>(fut: F) -> error::Result<()>
where
    F: std::future::Future<Output = error::Result<()>> + Send,
{
    runtime::tokio::block_on(fut)
}
