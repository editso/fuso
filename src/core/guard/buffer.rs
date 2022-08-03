use std::collections::VecDeque;

#[derive(Debug, Default)]
pub struct Buffer<T> {
    len: usize,
    buf: VecDeque<Vec<T>>,
}

impl<T> Buffer<T>
where
    T: Clone,
{
    #[inline]
    pub fn new() -> Self {
        Self {
            len: 0,
            buf: Default::default(),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn clear(&mut self) {
        self.buf.clear();
        self.len = 0;
    }

    pub fn push_all(&mut self, data: Vec<T>) {
        let len = data.len();
        self.buf.push_back(data);
        self.len += len;
    }

    #[inline]
    pub fn push_back(&mut self, data: &[T]) {
        self.buf.push_back(data.to_vec());
        self.len += data.len();
    }

    #[inline]
    pub fn push_front(&mut self, data: &[T]) {
        self.buf.push_front(data.to_vec());
        self.len += data.len();
    }
}

impl Buffer<u8> {
    #[inline]
    pub fn read_to_buffer(&mut self, target: &mut [u8]) -> usize {
        let mut remaining = target.len();
        let mut read_len = 0;
        let buf = &mut self.buf;

        loop {
            if remaining == 0 {
                if self.len != 0 {
                    self.len -= read_len;
                }
                break read_len;
            }

            let data = buf.pop_front();

            if data.is_none() {
                remaining = 0;
                continue;
            }

            let data = unsafe { data.unwrap_unchecked() };

            if data.len() >= remaining {
                let n = unsafe {
                    let ptr = &mut target[read_len..];
                    std::ptr::copy((&data[..remaining]).as_ptr(), ptr.as_mut_ptr(), remaining);
                    remaining
                };

                remaining -= n;
                read_len += n;

                if data.len() != n {
                    buf.push_front(data[n..].to_vec())
                }
            } else {
                let n = unsafe {
                    let ptr = &mut target[read_len..];
                    std::ptr::copy(data.as_ptr(), ptr.as_mut_ptr(), data.len());
                    data.len()
                };

                read_len += n;
                remaining -= n;
            }
        }
    }
}
