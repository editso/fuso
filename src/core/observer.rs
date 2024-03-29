use std::{sync::Arc, time::Instant};

use crate::Address;

pub trait Observer {
    #[inline]
    fn on_connect(&self, address: &Address)
    where
        Self: Sized,
    {
        #[cfg(debug_assertions)]
        log::debug!("on_connect {}", address);
    }

    #[inline]
    fn on_handshake(&self, address: &Address)
    where
        Self: Sized,
    {
        #[cfg(debug_assertions)]
        log::debug!("on_handshake {}", address);
    }

    #[inline]
    fn on_stop(&self, time: Instant, address: &Address)
    where
        Self: Sized,
    {
        #[cfg(debug_assertions)]
        log::debug!("on_stop {:?} {}", time.elapsed(), address)
    }

    #[inline]
    fn on_error(&self, error: &crate::Error, address: &Address)
    where
        Self: Sized,
    {
        #[cfg(debug_assertions)]
        log::debug!("on_error {:?} {}", error, address)
    }
}

impl<T> Observer for Option<T>
where
    T: Observer,
{
    #[inline]
    fn on_connect(&self, address: &Address) {
        self.as_ref().map(|obs| obs.on_connect(address));
    }

    #[inline]
    fn on_error(&self, error: &crate::Error, address: &Address)
    where
        Self: Sized,
    {
        self.as_ref().map(|obs| obs.on_error(error, address));
    }

    #[inline]
    fn on_stop(&self, time: Instant, address: &Address) {
        self.as_ref().map(|obs| obs.on_stop(time, address));
    }
}

impl Observer for () {}

impl<T> Observer for Arc<T>
where
    T: Observer,
{
    #[inline]
    fn on_stop(&self, time: Instant, address: &Address) {
        (**self).on_stop(time, address)
    }

    #[inline]
    fn on_connect(&self, address: &Address) {
        (**self).on_connect(address)
    }

    #[inline]
    fn on_error(&self, error: &crate::Error, address: &Address)
    where
        Self: Sized,
    {
        (**self).on_error(error, address)
    }
}
