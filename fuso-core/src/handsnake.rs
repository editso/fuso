use async_trait::async_trait;
use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Debug, Clone)]
pub struct Handsnake {
    pub prefix: String,
    pub suffix: String,
    pub max_bytes: usize,
    pub write: String,
}

#[async_trait]
pub trait HandsnakeEx {
    async fn handsnake(&mut self, handsnake: &Handsnake) -> std::io::Result<bool>;
    async fn write_handsnake(&mut self, handsnake: &Handsnake) -> std::io::Result<()>;
}

#[async_trait]
impl<T> HandsnakeEx for T
where
    T: AsyncRead + AsyncWrite + Unpin + Send + Sync + 'static,
{
    async fn handsnake(&mut self, handsnake: &Handsnake) -> std::io::Result<bool> {
        let len = handsnake.max_bytes;

        let mut buf = Vec::new();
        buf.resize(len, 0);

        let n = self.read(&mut buf).await?;

        buf.truncate(n);

        if buf.starts_with(handsnake.prefix.as_bytes())
            && buf.ends_with(handsnake.suffix.as_bytes())
        {
            log::info!(
                "[handsnake] prefix={}, suffix={}, max_bytes={}",
                handsnake.prefix.escape_default(),
                handsnake.suffix.escape_default(),
                handsnake.max_bytes
            );

            self.write_all(handsnake.write.as_bytes()).await?;

            log::debug!("[handsnake] write {}", handsnake.write.escape_default());

            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn write_handsnake(&mut self, handsnake: &Handsnake) -> std::io::Result<()> {
        self.write_all(handsnake.write.as_bytes()).await?;

        let len = handsnake.max_bytes;

        let mut buf = Vec::new();
        buf.resize(len, 0);

        let n = self.read(&mut buf).await?;

        buf.truncate(n);

        if buf.starts_with(handsnake.prefix.as_bytes())
            && buf.ends_with(handsnake.suffix.as_bytes())
        {
            log::info!(
                "[handsnake] prefix={}, suffix={}, max_bytes={}",
                handsnake.prefix.escape_default(),
                handsnake.suffix.escape_default(),
                handsnake.max_bytes
            );

            log::debug!("[handsnake] recv {}", handsnake.write.escape_default());
        }

        Ok(())
    }
}
