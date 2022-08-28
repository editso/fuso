use std::sync::Arc;

use crate::{Address, Error};

use super::server;

pub trait PenetrateWebhook {
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

    fn on_pen_stop(
        &self,
        client: &Address,
        visit: &Address,
        server: &Address,
        _: &server::Config,
    ) where
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

    fn on_pen_error(&self, client: &Address, _: &server::Config, error: &Error)
    where
        Self: Sized,
    {
        log::debug!("on_pen_error {} {}", client, error);
    }
}

impl PenetrateWebhook for () {}

impl<T> PenetrateWebhook for Arc<T>
where
    T: PenetrateWebhook,
{
    fn on_pen_error(&self, client: &Address, config: &server::Config, error: &Error)
    where
        Self: Sized,
    {
        (**self).on_pen_error(client, config, error)
    }

    fn on_pen_route(&self, client: &Address, from: &Address, to: &Address)
    where
        Self: Sized,
    {
        (**self).on_pen_route(client, from, to)
    }

    fn on_pen_start(
        &self,
        client: &Address,
        visit: &Address,
        server: &Address,
        config: &server::Config,
    ) where
        Self: Sized,
    {
        (**self).on_pen_start(client, visit, server, config)
    }

    fn on_pen_stop(
        &self,
        client: &Address,
        visit: &Address,
        server: &Address,
        config: &server::Config,
    ) where
        Self: Sized,
    {
        (**self).on_pen_stop(client, visit, server, config)
    }
}

impl<T> PenetrateWebhook for Option<T>
where
    T: PenetrateWebhook,
{
    fn on_pen_error(&self, client: &Address, config: &server::Config, error: &Error)
    where
        Self: Sized,
    {
        self.as_ref()
            .map(|obs| obs.on_pen_error(client, config, error));
    }

    fn on_pen_route(&self, client: &Address, from: &Address, to: &Address)
    where
        Self: Sized,
    {
        self.as_ref().map(|obs| obs.on_pen_route(client, from, to));
    }

    fn on_pen_start(
        &self,
        client: &Address,
        visit: &Address,
        server: &Address,
        config: &server::Config,
    ) where
        Self: Sized,
    {
        self.as_ref()
            .map(|obs| obs.on_pen_start(client, visit, server, config));
    }

    fn on_pen_stop(
        &self,
        client: &Address,
        visit: &Address,
        server: &Address,
        config: &server::Config,
    ) where
        Self: Sized,
    {
        self.as_ref()
            .map(|obs| obs.on_pen_stop(client, visit, server, config));
    }
}
