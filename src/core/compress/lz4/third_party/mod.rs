/// lz4: https://github.com/lz4/lz4
use std::ffi::c_void;

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

    pub(super) fn LZ4_createStream() -> *mut c_void;

    pub(super) fn LZ4_freeStream(streamPtr: *mut c_void) -> i32;

    pub(super) fn LZ4_resetStream_fast(streamPtr: *mut c_void);

    pub(super) fn LZ4_createStreamDecode() -> *mut c_void;

    pub(super) fn LZ4_freeStreamDecode(LZ4_stream: *mut c_void) -> i32;

    pub(super) fn LZ4_compressBound(inputSize: i32) -> i32;

    pub(super) fn LZ4_compress_fast_continue(
        streamPtr: *mut c_void,
        src: *const u8,
        dst: *mut u8,
        srcSize: i32,
        dstCapacity: i32,
        acceleration: i32,
    ) -> i32;

    pub(super) fn LZ4_decompress_safe_continue(
        LZ4_streamDecode: *mut c_void,
        src: *const u8,
        dst: *mut u8,
        srcSize: i32,
        dstCapacity: i32,
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
