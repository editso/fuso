fn main() {
    
    cc::Build::new()
        .file(concat!("src/core/compress/lz4/third_party/lib/", "lz4.c"))
        .include("src/core/compress/lz4/third_party/lib/")
        .compile("lib_third_party_compress_lz4")
}
