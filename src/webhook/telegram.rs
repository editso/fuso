use serde_json::json;

use crate::{penetrate::PenetrateWebhook, Webhook};

use super::hyper::Post;

pub struct Telegram<E>(super::hyper::HTTPWebhook<E>);

impl<E> Telegram<E> {
    pub fn new(bot_token: String, chat_id: String, executor: E, server: Option<String>) -> Self {
        Self(super::hyper::HTTPWebhook {
            executor,
            server: format!(
                "{}/{}/sendMessage",
                server.unwrap_or(String::from("api.telegram.org")),
                bot_token
            ),
            format: super::hyper::Format::Html,
            enable_ssl: true,
            method: super::hyper::Method::Post(Post::Json {
                field: "text".to_owned(),
                json: json!({
                    "chat_id": chat_id,
                    "parseMode": "HTML"
                }),
            }),
            headers: vec![("Content-Type", "application/json")]
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect(),
        })
    }
}

impl<E> Webhook for Telegram<E> {
    fn on_connect(&self, address: &crate::Address)
    where
        Self: Sized,
    {
        self.0.on_connect(address)
    }

    fn on_error(&self, error: &crate::Error, address: &crate::Address)
    where
        Self: Sized,
    {
        self.0.on_error(error, address)
    }
    fn on_stop(&self, time: std::time::Instant, address: &crate::Address)
    where
        Self: Sized,
    {
        self.0.on_stop(time, address)
    }

    fn on_handshake(&self, address: &crate::Address)
    where
        Self: Sized,
    {
        self.0.on_handshake(address)
    }
}

impl<E> PenetrateWebhook for Telegram<E> {
    fn on_pen_error(
        &self,
        client: &crate::Address,
        config: &crate::penetrate::server::Config,
        error: &crate::Error,
    ) where
        Self: Sized,
    {
        self.0.on_pen_error(client, config, error)
    }

    fn on_pen_route(&self, client: &crate::Address, from: &crate::Address, to: &crate::Address)
    where
        Self: Sized,
    {
        self.0.on_pen_route(client, from, to)
    }

    fn on_pen_stop(
        &self,
        client: &crate::Address,
        visit: &crate::Address,
        server: &crate::Address,
        config: &crate::penetrate::server::Config,
    ) where
        Self: Sized,
    {
        self.0.on_pen_stop(client, visit, server, config)
    }

    fn on_pen_start(
        &self,
        client: &crate::Address,
        visit: &crate::Address,
        server: &crate::Address,
        config: &crate::penetrate::server::Config,
    ) where
        Self: Sized,
    {
        self.0.on_pen_start(client, visit, server, config)
    }
}
