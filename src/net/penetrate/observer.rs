use std::sync::Arc;

use crate::{Address, Error};

use super::server;

pub trait PenetrateObserver {
    fn on_pen_start(
        &self,
        client: &Address,
        visit: &Address,
        server: &Address,
        config: &server::Config,
    ) where
        Self: Sized,
    {
        log::debug!(
            "on_pen_start client: {}, visit: {}, server: {}, config: {:#?}",
            client,
            visit,
            server,
            config
        );
    }

    fn on_pen_stop(&self, client: &Address, visit: &Address, server: &Address)
    where
        Self: Sized,
    {
        log::debug!(
            "on_pen_stop client: {}, visit: {}, server: {}",
            client,
            visit,
            server
        )
    }

    fn on_pen_route(&self, client: &Address, from: &Address, to: &Address)
    where
        Self: Sized,
    {
        log::debug!(
            "on_pen_route client: {}, from: {}, to: {}",
            client,
            from,
            to
        );
    }

    fn on_pen_error(&self, client: &Address, error: &Error)
    where
        Self: Sized,
    {
        log::debug!("on_pen_error {} {}", client, error);
    }
}

impl PenetrateObserver for () {}

impl<T> PenetrateObserver for Arc<T> where T: PenetrateObserver {}

impl<T> PenetrateObserver for Option<T> where T: PenetrateObserver {}
