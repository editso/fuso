mod third_party;

use std::{ffi::c_void, pin::Pin, task::Poll};

use crate::{guard::buffer::Buffer, AsyncRead, AsyncWrite, Lz4Err, NetSocket, ReadBuf};

use super::{Decoder, Encoder};

/// ref https://github.com/lz4/lz4/blob/dev/examples/blockStreaming_ringBuffer.c
pub struct Lz4Compress<T> {
    lz4_stream: T,
    /// 上层读取时，如果解压后的数据大于了提供的缓冲区时，数据将缓存到此处
    lz4_rbuf: Buffer<u8>,
    /// 压缩缓冲区
    lz4_ebuf: Vec<u8>,
    /// 解压缓冲区
    lz4_dbuf: Vec<u8>,
    /// 需要从lz4_stream读取的数据
    lz4_dneed: usize,
    /// lz4_dbuf偏移
    lz4_doffset: usize,
    /// dbuf 初始化状态
    lz4_dinit: bool,
    /// 环形压缩缓冲
    lz4_ering_buf: Vec<u8>,
    /// 环形解压缓冲
    lz4_dring_buf: Vec<u8>,
    /// 偏移量
    lz4_dring_offset: usize,
    /// 偏移量
    lz4_ering_offset: usize,
    /// 相对lz4_ebuf写入偏移
    lz4_woffset: usize,
    /// 对于write提供的buf偏移
    lz4_compressed_offset: usize,
    /// 一次最多能压缩的数据
    lz4_compress_max_bytes: usize,
    /// ----
    lz4_encode_insptr: *mut c_void,
    /// ----
    lz4_decode_insptr: *mut c_void,
}

unsafe impl<T> Send for Lz4Compress<T> {}
unsafe impl<T> Sync for Lz4Compress<T> {}

impl<T> NetSocket for Lz4Compress<T>
where
    T: NetSocket,
{
    fn local_addr(&self) -> crate::Result<crate::Address> {
        self.lz4_stream.local_addr()
    }

    fn peer_addr(&self) -> crate::Result<crate::Address> {
        self.lz4_stream.peer_addr()
    }
}

impl<T> Lz4Compress<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    pub fn new(lz4_stream: T) -> Self {
        let lz4_ebuf = Vec::with_capacity(unsafe { third_party::LZ4_compressBound(1024) as usize });
        let lz4_dbuf = Vec::with_capacity(unsafe { third_party::LZ4_compressBound(1024) as usize });
        let mut lz4_ering_buf = Vec::with_capacity(1024 * 8 + 1024);
        let mut lz4_dring_buf = Vec::with_capacity(1024 * 8 + 1024 + 1024);

        unsafe {
            // 临时使用，未来将改为配置文件的形式
            lz4_ering_buf.set_len(1024 * 8 + 1024);
            lz4_dring_buf.set_len(1024 * 8 + 1024 + 1024);
        }

        log::debug!("use lz4 compression");

        Self {
            lz4_ebuf,
            lz4_dinit: false,
            lz4_dneed: 0,
            lz4_dbuf,
            lz4_woffset: 0,
            lz4_stream,
            lz4_doffset: 0,
            lz4_dring_buf,
            lz4_ering_buf,
            lz4_rbuf: Default::default(),
            lz4_ering_offset: 0,
            lz4_dring_offset: 0,
            lz4_compressed_offset: 0,
            lz4_compress_max_bytes: 1024,
            lz4_decode_insptr: unsafe { third_party::LZ4_createStreamDecode() },
            lz4_encode_insptr: unsafe { third_party::LZ4_createStream() },
        }
    }
}

impl<T> Drop for Lz4Compress<T> {
    fn drop(&mut self) {
        if !self.lz4_encode_insptr.is_null() {
            unsafe {
                third_party::LZ4_freeStream(self.lz4_encode_insptr);
                self.lz4_encode_insptr = std::ptr::null_mut();
            }
        }

        if !self.lz4_decode_insptr.is_null() {
            unsafe {
                third_party::LZ4_freeStreamDecode(self.lz4_decode_insptr);
                self.lz4_encode_insptr = std::ptr::null_mut();
            }
        }
    }
}

impl<T> AsyncRead for Lz4Compress<T>
where
    T: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> std::task::Poll<crate::Result<usize>> {
        if !self.lz4_rbuf.is_empty() {
            let unfilled = buf.initialize_unfilled();
            let n = self.lz4_rbuf.read_to_buffer(unfilled);
            buf.advance(n);

            return Poll::Ready(Ok(n));
        }

        if self.lz4_decode_insptr.is_null() {
            return Poll::Ready(Err(Lz4Err::EncodeReleased.into()));
        }

        self.poll_decode_read(cx, buf)
    }
}

impl<T> AsyncWrite for Lz4Compress<T>
where
    T: AsyncWrite + Unpin,
{
    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        Pin::new(&mut self.lz4_stream).poll_close(cx)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        Pin::new(&mut self.lz4_stream).poll_flush(cx)
    }

    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::Result<usize>> {
        if self.lz4_encode_insptr.is_null() {
            return Poll::Ready(Err(Lz4Err::EncodeReleased.into()));
        }

        self.poll_encode_write(cx, buf)
    }
}

impl<T> Encoder for Lz4Compress<T>
where
    T: AsyncWrite + Unpin,
{
    fn poll_encode_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context,
        buf: &[u8],
    ) -> std::task::Poll<crate::Result<usize>> {
        loop {
            let woffset = self.lz4_woffset;
            let max_bytes = self.lz4_compress_max_bytes;
            let mut ebuf = std::mem::replace(&mut self.lz4_ebuf, unsafe {
                Vec::with_capacity(third_party::LZ4_compressBound(max_bytes as i32) as usize)
            });

            if ebuf.is_empty() {
                // ebuf.len() == 0 说明是第一次写, 进行初始化
                // 用户提供的数据有可能会超过lz4_compress_max_bytes, 那么这里就需要进行分片压缩
                // 还挺复杂...

                let need_compress = &buf[self.lz4_compressed_offset..];
                let need_compress_len = need_compress.len() + 4;
                let compress_len = {
                    if need_compress_len > ebuf.capacity() {
                        let diff = need_compress_len - ebuf.capacity();
                        need_compress_len - diff - 4
                    } else {
                        need_compress_len - 4
                    }
                };

                unsafe {
                    let lz4_encode_ins = self.lz4_encode_insptr;
                    let need_ring_offset = self.lz4_ering_offset;
                    let need_ring_buf = &mut self.lz4_ering_buf[need_ring_offset..];

                    std::ptr::copy(
                        need_compress.as_ptr(),
                        need_ring_buf.as_mut_ptr(),
                        compress_len,
                    );

                    let compressed_len = third_party::LZ4_compress_fast_continue(
                        lz4_encode_ins,
                        need_ring_buf.as_ptr(),
                        ebuf.as_mut_ptr().add(4),
                        compress_len as i32,
                        ebuf.capacity() as i32,
                        0,
                    );

                    log::debug!(
                        "total {}bytes, compressed: {}bytes, need: {}",
                        buf.len(),
                        compressed_len,
                        need_compress_len
                    );

                    if compressed_len <= 0 {
                        return Poll::Ready(Err(Lz4Err::Compress.into()));
                    }

                    let compress_len_bytes = compressed_len.to_le_bytes();

                    std::ptr::copy(compress_len_bytes.as_ptr(), ebuf.as_mut_ptr(), 4);

                    ebuf.set_len(compressed_len as usize + 4);
                }

                self.lz4_compressed_offset += compress_len as usize;
                self.lz4_ering_offset = {
                    if self.lz4_ering_offset + compress_len
                        >= self.lz4_ering_buf.capacity() - self.lz4_compress_max_bytes
                    {
                        0
                    } else {
                        self.lz4_ering_offset + compress_len
                    }
                };
            }

            let n = match Pin::new(&mut self.lz4_stream).poll_write(cx, &ebuf[woffset..])? {
                Poll::Ready(n) => n,
                Poll::Pending => {
                    drop(std::mem::replace(&mut self.lz4_ebuf, ebuf));
                    return Poll::Pending;
                }
            };

            if n == 0 {
                return Poll::Ready(Ok(n));
            }

            self.lz4_woffset += n;

            if self.lz4_woffset == ebuf.len() && self.lz4_compressed_offset == buf.len() {
                self.lz4_woffset = 0;
                self.lz4_compressed_offset = 0;
                break Poll::Ready(Ok(buf.len()));
            } else {
                self.lz4_ebuf.extend(&ebuf[n..]);
            }
        }
    }
}

impl<T> Decoder for Lz4Compress<T>
where
    T: AsyncRead + Unpin,
{
    fn poll_decode_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context,
        buf: &mut crate::ReadBuf<'_>,
    ) -> std::task::Poll<crate::Result<usize>> {
        loop {
            let max_bytes = self.lz4_compress_max_bytes;
            let mut dbuf = std::mem::replace(&mut self.lz4_dbuf, unsafe {
                Vec::with_capacity(third_party::LZ4_compressBound(max_bytes as i32) as usize)
            });

            let mut need_size = self.lz4_dneed;
            let mut need_offset = self.lz4_doffset;

            if need_size == 0 {
                // 读入前4个字节获取到压缩块大小
                unsafe { dbuf.set_len(4) };
                self.lz4_dneed = 4;
                need_offset = 0;
            } else if need_size == need_offset && !self.lz4_dinit {
                // 获取到压缩块大小，继续读取....
            
                need_size =
                    unsafe { *(dbuf.as_ptr() as *const i32).as_ref().unwrap_unchecked() } as usize;

                if need_size <= 0 {
                    return Poll::Ready(Err(Lz4Err::Decompress.into()));
                }

                if need_size > max_bytes {
                    return Poll::Ready(Err(Lz4Err::Decompress.into()));
                }

                unsafe {
                    dbuf.set_len(need_size);
                }

                need_offset = 0;

                self.lz4_dneed = dbuf.len();
                self.lz4_doffset = 0;
                self.lz4_dinit = true;
            } else if need_size == need_offset && self.lz4_dinit {
                let dring_offset = self.lz4_dring_offset;
                let lz4_decode_ins = self.lz4_decode_insptr;

                let decompress_size = unsafe {
                    let lz4_compress_max_bytes = self.lz4_compress_max_bytes;
                    let dring_buf = &mut self.lz4_dring_buf[dring_offset..];
                    third_party::LZ4_decompress_safe_continue(
                        lz4_decode_ins,
                        dbuf.as_ptr(),
                        dring_buf.as_mut_ptr(),
                        dbuf.len() as i32,
                        lz4_compress_max_bytes as i32,
                    )
                };

                if decompress_size <= 0 {
                    // 解压失败了 呜呜呜.....
                    return Poll::Ready(Err(Lz4Err::Decompress.into()));
                }

                let decompress_size = decompress_size as usize;

                self.lz4_dinit = false;
                self.lz4_dneed = 0;
                self.lz4_doffset = 0;

                self.lz4_dring_offset = {
                    if self.lz4_dring_offset + decompress_size
                        >= self.lz4_dring_buf.capacity() - self.lz4_compress_max_bytes
                    {
                        0
                    } else {
                        self.lz4_dring_offset + decompress_size
                    }
                };

                let unfilled_len = buf.remaining();
                let dring_buf = &self.lz4_dring_buf[dring_offset..];

                if unfilled_len < decompress_size {
                    // 提供的缓冲区不够存放当前解压后的数据，临时存放到 lz4_rbuf

                    unsafe {
                        let unfilled = buf.initialize_unfilled();
                        std::ptr::copy(dring_buf.as_ptr(), unfilled.as_mut_ptr(), unfilled_len)
                    }

                    // 区间小细节 ..decompress_size
                    let rem = dring_buf[unfilled_len..decompress_size].to_vec();

                    log::debug!(
                        "buffer {}bytes, decompressed: {}bytes, rem: {}bytes",
                        buf.len(),
                        decompress_size,
                        rem.len()
                    );

                    self.lz4_rbuf.push_all(rem);
                    buf.advance(unfilled_len);

                    return Poll::Ready(Ok(unfilled_len));
                } else {
                    // 提供的缓冲区完全够大，直接copy

                    unsafe {
                        let unfilled = buf.initialize_unfilled();
                        std::ptr::copy(dring_buf.as_ptr(), unfilled.as_mut_ptr(), decompress_size)
                    }

                    buf.advance(decompress_size);

                    log::debug!(
                        "buffer {}bytes, decompressed: {}bytes",
                        buf.len(),
                        decompress_size
                    );

                    return Poll::Ready(Ok(decompress_size));
                }
            }

            let mut read_buf = ReadBuf::new(&mut dbuf[need_offset..]);

            let n = match Pin::new(&mut self.lz4_stream).poll_read(cx, &mut read_buf)? {
                Poll::Ready(n) => n,
                Poll::Pending => {
                    // 未就绪，直接返回 等待下层 wake
                    drop(std::mem::replace(&mut self.lz4_dbuf, dbuf));
                    return Poll::Pending;
                }
            };

            if n == 0 {
                self.lz4_dneed = 0;
                self.lz4_doffset = 0;
                return Poll::Ready(Ok(0));
            } else {
                self.lz4_doffset += n;
                drop(std::mem::replace(&mut self.lz4_dbuf, dbuf));
            }
        }
    }
}

#[cfg(test)]
#[allow(unused)]
mod tests {

    use std::time::Duration;

    use tokio::net::{TcpListener, TcpStream};

    use super::Lz4Compress;
    use crate::{
        ext::AsyncWriteExt,
        protocol::{AsyncRecvPacket, Poto, ToPacket},
        r#async::ext::AsyncReadExt,
        time,
    };

    fn init_logger() {
        env_logger::Builder::default()
            .filter_module("fuso", log::LevelFilter::Debug)
            .init();
    }

    #[test]
    fn test_lz4_client() {
        init_logger();

        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let tcp = TcpStream::connect("127.0.0.1:6666").await.unwrap();
                let mut lz4 = Lz4Compress::new(tcp);

                loop {
                    let mut text = String::new();
                    time::sleep(Duration::from_secs(1)).await;
                    let data = Poto::Ping.to_packet_vec();

                    lz4.write_all(&data).await.unwrap();

                    let mut buf = [0u8; 1024];
                    let n = lz4.recv_packet().await.unwrap();
                    println!("recv {:?}", n);
                }
            });
    }

    #[test]
    fn test_lz4_server() {
        init_logger();

        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let tcp = TcpListener::bind("127.0.0.1:6666").await.unwrap();

                loop {
                    let (tcp, _) = tcp.accept().await.unwrap();
                    let mut lz4 = Lz4Compress::new(tcp);
                    loop {
                        let mut buf = [0u8; 1024];
                        let n = lz4.recv_packet().await.unwrap();
                        println!("recv {:?}", n);

                        lz4.write_all(&n.encode()).await.unwrap();
                    }
                }
            });
    }

    #[test]
    fn test_lz4_compress() {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let file = tokio::fs::File::create("target/compress.txt")
                    .await
                    .unwrap();

                let mut lz4 = Lz4Compress::new(file);

                lz4.write_all(b"fskhadfhkjsdhkjsdhfkdj").await.unwrap();

                lz4.write_all(b"gfdgfdgsdh111").await.unwrap();

                lz4.write_all(b"222fdfsdfdsf2222222222211111111999999999")
                    .await
                    .unwrap();

                lz4.close().await.unwrap();

                let file = tokio::fs::File::open("target/compress.txt").await.unwrap();
                let mut lz4 = Lz4Compress::new(file);

                let mut buf = Vec::new();

                buf.resize(60, 0);

                lz4.read(&mut buf).await.unwrap();
                println!("{:?}", String::from_utf8_lossy(&buf));
                let n = lz4.read(&mut buf).await.unwrap();
                println!("{} {:?}", n, String::from_utf8_lossy(&buf));
                lz4.read(&mut buf).await.unwrap();
                println!("{:?}", String::from_utf8_lossy(&buf));
                lz4.read(&mut buf).await.unwrap();
                println!("{:?}", String::from_utf8_lossy(&buf));
            })
    }
}
