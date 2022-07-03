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
        
        #[repr(C)]
        struct a{
            magic: [u8;4],
            len: u32
        }

        let b  = 10u32.to_le_bytes();

        let b = make_packet(b.to_vec()).encode();

        unsafe{

            let a = (b.as_ptr() as *const _ as *const a).as_ref().unwrap();
            println!("{:?}", String::from_utf8_lossy(&a.magic));

            println!("{:?}", a.len);
        }

    }
}