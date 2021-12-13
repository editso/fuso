use aes::{cipher::generic_array::GenericArray, Aes128, NewBlockCipher, Aes256};
use block_modes::{block_padding::Pkcs7, Cbc};
use fuso_core::cipher::Cipher;
use hex_literal::hex;

pub struct AesCrypt(String);

impl Cipher for AesCrypt {
    fn poll_decrypt(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        data: &[u8],
    ) -> std::task::Poll<std::io::Result<Vec<u8>>> {
        todo!()
    }

    fn poll_encrypt(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        data: &[u8],
    ) -> std::task::Poll<std::io::Result<Vec<u8>>> {
        todo!()
    }
}
