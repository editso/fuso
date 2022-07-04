mod r#async;
mod core;
mod error;
mod runtime;
mod net;

pub mod client;
pub mod server;

#[cfg(any(feature = "fuso-web", feature = "fuso-api"))]
pub mod http;

pub use net::*;
pub use crate::core::*;
pub use crate::error::*;
pub use crate::r#async::*;
pub use crate::runtime::*;


#[cfg(test)]
mod test{
    use crate::protocol::make_packet;


    #[test]
    fn test_ptr(){
        
        let a = Ok::<_,std::io::Error>(1);;
        unsafe{
            a.unwrap_err_unchecked();
        }
    }
}