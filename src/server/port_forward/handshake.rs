use crate::core::{
    handshake::Handshaker,
    io::AsyncReadExt,
    io::{AsyncRead, AsyncWrite},
};

pub struct Rc4MagicHandshake {
    pub expect: u32,
    pub secret: Vec<u8>,
}

impl<T> Handshaker<T> for Rc4MagicHandshake
where
    T: AsyncRead + AsyncWrite + Send + Unpin,
{
    fn do_handshake<'a>(
        &'a mut self,
        stream: &'a mut T,
    ) -> crate::core::BoxedFuture<'a, crate::error::Result<()>> {
        Box::pin(async move {
            let mut buf = [0u8; 1024];

            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use rc4::{KeyInit, StreamCipher};

    #[test]
    fn test_rc4_magic_handshake() {
        // let s = [u32;16];

        // let mut a = rc4::Rc4::new_from_slice(&s).unwrap();
        // let mut b = b"11111".to_vec();

        // println!("{:?}", b);
        // let mut c = Vec::new();

        // a.apply_keystream( &mut c);

        // println!("{:?}", b);
        // let mut a = rc4::Rc4::new(b"hello world".into());
        // a.apply_keystream(&mut b);

        // println!("{:?}", b);
    }
}
