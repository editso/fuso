use serde::{Serialize, Deserialize};

use crate::{penetrate::PenetrateWebhook, Webhook};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Format {
    Html,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Post {
    Text,
    Json {
        field: String,
        json: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Method {
    Get(String, Option<Vec<(String, String)>>),
    Post(Post),
}

pub struct HTTPWebhook<E> {
    pub server: String,
    pub format: Format,
    pub method: Method,
    pub headers: Vec<(String, String)>,
    pub enable_ssl: bool,
    pub executor: E,
}

impl Default for Format {
    fn default() -> Self {
        Format::Json
    }
}

impl Default for Method {
    fn default() -> Self {
        Self::Get(format!("text"), None)
    }
}

impl<E> Webhook for HTTPWebhook<E> {
    fn on_connect(&self, address: &crate::Address)
    where
        Self: Sized,
    {
        unimplemented!()
    }

    fn on_handshake(&self, address: &crate::Address)
    where
        Self: Sized,
    {
        unimplemented!()
    }

    fn on_error(&self, error: &crate::Error, address: &crate::Address)
    where
        Self: Sized,
    {
        unimplemented!()
    }

    fn on_stop(&self, time: std::time::Instant, address: &crate::Address)
    where
        Self: Sized,
    {
        unimplemented!()
    }
}

impl<E> PenetrateWebhook for HTTPWebhook<E> {
    fn on_pen_error(
        &self,
        client: &crate::Address,
        _: &crate::penetrate::server::ClientConfig,
        error: &crate::Error,
    ) where
        Self: Sized,
    {
        match self.format {
            Format::Html => todo!(),
            Format::Json => todo!(),
        }
        unimplemented!()
    }

    fn on_pen_route(&self, client: &crate::Address, from: &crate::Address, to: &crate::Address)
    where
        Self: Sized,
    {
        unimplemented!()
    }

    fn on_pen_start(
        &self,
        client: &crate::Address,
        visit: &crate::Address,
        server: &crate::Address,
        config: &crate::penetrate::server::ClientConfig,
    ) where
        Self: Sized,
    {
        unimplemented!()
    }

    fn on_pen_stop(
        &self,
        client: &crate::Address,
        visit: &crate::Address,
        server: &crate::Address,
        _: &crate::penetrate::server::ClientConfig,
    ) where
        Self: Sized,
    {
        unimplemented!()
    }
}
