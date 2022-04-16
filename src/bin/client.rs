#[cfg(feature = "fuso-rt-tokio")]
#[tokio::main]
async fn main() {
    use fuso::{DefaultExecutor, DefaultRegister, Executor};
}

#[cfg(feature = "fuso-web")]
#[tokio::main]
async fn main() {}

#[cfg(feature = "fuso-api")]
#[tokio::main]
async fn main() {}

#[cfg(feature = "fuso-rt-smol")]
fn main() {}

#[cfg(feature = "fuso-rt-custom")]
fn main() {}
