use std::{sync::Arc, task::Waker};

use parking_lot::Mutex;

use crate::error;

mod pool;

pub enum ValState<V> {
  Ok(V),
  Nil,
  Invalid
}

pub struct Getter<V> {
    val: Arc<Mutex<Option<ValState<V>>>>,
    waker: Arc<Mutex<Waker>>,
}

pub struct Setter<V> {
    val: Arc<Mutex<Option<ValState<V>>>>,
    waker: Arc<Mutex<Waker>>,
}

impl<V> Setter<V> {
    pub fn set(self, val: V) -> error::Result<()> {
        unimplemented!()
    }

    pub fn invalid(self) {}
}

impl<V> std::future::Future for Getter<V> {
    type Output = error::Result<V>;
    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let val = self.val.lock();

        

        unimplemented!()
    }
}


pub fn setter<V>() -> (Setter<V>, Getter<V>) {
    unimplemented!()
}
