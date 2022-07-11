mod r#async;
mod core;
mod error;
mod net;
mod runtime;

pub mod client;
pub mod server;

#[cfg(any(feature = "fuso-web", feature = "fuso-api"))]
pub mod http;

pub use crate::core::*;
pub use crate::error::*;
pub use crate::r#async::*;
pub use crate::runtime::*;
pub use net::*;

#[cfg(test)]
mod test {
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
        net::SocketAddr,
    };

    #[test]
    fn t() {
        let addr = SocketAddr::from(([0, 0, 0, 0], 0));
        let mut hash = DefaultHasher::new();
        addr.hash(&mut hash);

        println!("{}", hash.finish())
    }
}
