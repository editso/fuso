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
    fn test() {
        unsafe {
            let c = [102u8, 117, 115, 111, 26, 0, 3, 0];

            #[allow(unused)]
            #[repr(C)]
            struct Head {
                magic: [u8; 4],
                data_len: u32,
            }

            let head = (c.as_ptr() as *const Head).as_ref().unwrap();

            println!("{}", head.data_len)
        }
    }
}
