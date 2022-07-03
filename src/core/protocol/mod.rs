#[cfg(feature = "fuso-serde")]
mod serde;
#[cfg(feature = "fuso-serde")]
pub use self::serde::*;

#[cfg(not(feature = "fuso-serde"))]
mod local;

#[cfg(not(feature = "fuso-serde"))]
pub use local::*;

pub mod proto;
pub use proto::*;

mod stream;

/// f => 0x66
/// u => 0x75
/// s => 0x73
/// o => 0x6f
pub const MAGIC: [u8; 4] = [0x66, 0x75, 0x73, 0x6f];

pub use self::stream::*;

pub fn head_size() -> usize {
    8
}

pub fn good(magic_buf: &[u8; 4]) -> bool {
    MAGIC.eq(magic_buf)
}

pub fn empty() -> Packet {
    Packet {
        magic: u32::from_be_bytes(MAGIC),
        data_len: 0,
        payload: vec![],
    }
}

pub fn make_packet(buf: Vec<u8>) -> Packet {
    Packet {
        magic: u32::from_be_bytes(MAGIC),
        data_len: buf.len() as u32,
        payload: buf,
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn magic() {
        let buf = b"o";
        println!("{:x?}", buf);
    }
}
