mod fuso_clap;

#[cfg(feature = "fuso-clap")]
use fuso_clap as fuc;


fn main() -> fuso::Result<()> {
    fuso::block_on(fuc::fuso_main())
}
