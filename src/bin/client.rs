use fuso::penetrate::client::PenetrateClientFactory;

fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .default_format()
        .format_module_path(false)
        .init();

    tokio::runtime::Runtime::new()
        .expect("failed to runtime")
        .block_on(async move {
            fuso::builder_client_with_tokio()
                .build(([0, 0, 0, 0], 8888), PenetrateClientFactory {})
                .run()
                .await
        });
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
