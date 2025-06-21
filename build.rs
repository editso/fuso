fn main() {
    // cc::Build::new()
    //     .file(concat!("src/core/compress/lz4/third_party/lib/", "lz4.c"))
    //     .include("src/core/compress/lz4/third_party/lib/")
    //     .compile("lib_third_party_compress_lz4")
    compile_tty_and_generate_bindings();
}

fn compile_tty_and_generate_bindings() {
    let manifest_root = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let tty_source_dir = format!("{manifest_root}/src/toy/pty/c");

    println!("cargo:rerun-if-changed={tty_source_dir}");

    cc::Build::new()
        .file(format!("{tty_source_dir}/pty_impl.c"))
        .include(&tty_source_dir)
        .warnings(false)
        .compile("pty");
}
