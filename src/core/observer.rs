use std::{sync::Arc, time::Instant};

use crate::Address;

pub trait Observer {
    fn on_connect(&self, address: &Address)
    where
        Self: Sized,
    {
        log::debug!("on_connect {}", address);
    }

    fn on_handshake(&self, address: &Address)
    where
        Self: Sized,
    {
        log::debug!("on_handshake {}", address);
    }

    fn on_stop(&self, time: Instant, address: &Address)
    where
        Self: Sized,
    {
        log::debug!("on_stop {:?} {}", time.elapsed(), address)
    }

    fn on_error(&self, address: &Address)
    where
        Self: Sized,
    {
        log::debug!("on_error {}", address)
    }
}

impl<T> Observer for Option<T>
where
    T: Observer,
{
    fn on_connect(&self, address: &Address) {
        self.as_ref().map(|obs| obs.on_connect(address));
    }

    fn on_error(&self, address: &Address) {
        self.as_ref().map(|obs| obs.on_error(address));
    }

    fn on_stop(&self, time: Instant, address: &Address) {
        self.as_ref().map(|obs| obs.on_stop(time, address));
    }
}

impl Observer for () {}

impl<T> Observer for Arc<T>
where
    T: Observer,
{
    fn on_stop(&self, time: Instant, address: &Address) {
        (**self).on_stop(time, address)
    }

    fn on_connect(&self, address: &Address) {
        (**self).on_connect(address)
    }

    fn on_error(&self, address: &Address) {
        (**self).on_error(address)
    }
}
