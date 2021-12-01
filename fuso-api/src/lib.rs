mod core;
mod buffer;
mod error;

pub use crate::buffer::*;
pub use crate::core::*;
pub use crate::error::*;
pub use async_trait::*;


#[cfg(test)]
mod tests {
    use crate::{core::Packet, now_mills};

    fn init_logger() {
        env_logger::builder()
            .filter_level(log::LevelFilter::Debug)
            .init();
    }

    #[test]
    fn test_time(){
        // let time = std::time::SystemTime::now()
        // .duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap();
        
         println!("{:?}", now_mills())
    }

    #[test]
    fn test_packet() {
        init_logger();

        let packet = Packet::new(1, "Hello".into());

        log::debug!("{:?}", Packet::size());
        log::debug!("{:?}", packet);

        assert_eq!(5, packet.get_len());

        let data = packet.encode();

        log::debug!("len: {}, raw: {:?}", data.len(), data);

        let packet = Packet::decode_data(&data).unwrap();

        log::debug!("data len: {}, {:?}", packet.get_len(), packet.get_data());
    }
}
