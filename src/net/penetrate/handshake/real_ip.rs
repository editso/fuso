use std::pin::Pin;

use crate::{FusoStream, Provider, ext::AsyncWriteExt};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub struct ProxyProtocol;


impl Provider<FusoStream> for ProxyProtocol {
    type Output = BoxedFuture<FusoStream>;

    fn call(&self, mut client: FusoStream) -> Self::Output {
        Box::pin(async move {
            // .....
            client.write_all(b"\x0D\x0A\x0D\x0A\x00\x0D\x0A\x51\x55\x49\x54\x0A\x02").await?;
            Ok(client)
        })
    }
}
