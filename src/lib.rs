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

    #[test]
    fn t(){
        println!("{}", u16::from_le_bytes([0, 53]))
    }
   
}
