/// lz4: https://github.com/lz4/lz4

#[link(name = "lib_third_party_compress_lz4", kind = "static")]
#[allow(unused)]
extern "C" {
    pub(super) fn LZ4_compress_default(
        src: *const u8,
        dst: *mut u8,
        srcSize: i32,
        dstCapacity: i32,
    ) -> i32;

    pub(super) fn LZ4_decompress_safe(
        src: *const u8,
        dst: *mut u8,
        compressedSize: i32,
        dstCapacity: i32,
    ) -> i32;

    pub(super) fn LZ4_compress_fast(
        src: *const u8,
        dst: *mut u8,
        srcSize: i32,
        dstCapacity: i32,
        acceleration: i32,
    ) -> i32;
}

#[cfg(test)]
mod tests {

    use super::LZ4_compress_default;

    #[test]
    fn test_lz4_compress() {
        unsafe {
            let ret = LZ4_compress_default(std::ptr::null(), std::ptr::null_mut(), 0, 0);
            assert_eq!(ret, 0)
        }
    }
}
