use crate::{
    ext::{AsyncReadExt, AsyncWriteExt},
    AsyncRead, AsyncWrite,
};

pub async fn copy<R, W>(mut reader: R, mut writer: W) -> crate::Result<()>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
{
    let mut buf = Vec::with_capacity(1500);

    unsafe {
        buf.set_len(1500);
    }

    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break Ok(());
        }

        writer.write_all(&buf).await?;
    }
}
