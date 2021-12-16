use async_trait::async_trait;
use fuso_api::FusoAuth;
use futures::{AsyncRead, AsyncWrite};

pub struct TokenAuth {
    token: String,
}

impl TokenAuth {
    pub fn new(token: &str) -> Self {
        Self {
            token: token.into(),
        }
    }
}



#[async_trait]
impl<T> FusoAuth<T> for TokenAuth
where
    T: AsyncRead + AsyncWrite + Unpin + Send + Sync + 'static,
{
    async fn auth(&self, _: &mut T) -> fuso_api::Result<()> {
        Ok(())
    }
}
