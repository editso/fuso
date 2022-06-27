mod r#async;
mod core;
mod error;
mod runtime;

pub mod client;
pub mod server;

#[cfg(any(feature = "fuso-web", feature = "fuso-api"))]
pub mod http;

pub mod net;


pub use self::core::*;
pub use self::error::*;
pub use self::r#async::*;
pub use self::runtime::*;

pub fn new_penetrate_server<S>() -> server::Builder<S>
{
    server::Builder {
        encryption: Default::default(),
        middleware: Default::default()
    }
}
