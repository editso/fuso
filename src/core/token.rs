use std::sync::Arc;

use parking_lot::Mutex;

#[derive(Debug, Clone)]
pub struct IncToken(Arc<Mutex<u64>>);

impl Default for IncToken {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(1)))
    }
}

impl IncToken {
    pub fn next<F>(&self, f: F) -> u64
    where
        F: Fn(u64) -> bool,
    {
        let mut inc_token = self.0.lock();

        loop {
            let cur_tok = *inc_token;

            let (cur_tok, overflow) = if f(cur_tok) {
                cur_tok.overflowing_add(1)
            } else {
                break cur_tok;
            };

            if overflow {
                *inc_token = 1;
            } else {
                *inc_token = cur_tok;
            }
        }
    }
}
