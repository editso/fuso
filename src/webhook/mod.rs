pub mod hyper;
pub mod telegram;

use std::{collections::HashMap, future::Future, pin::Pin};

use serde_json::json;

use crate::{penetrate::PenetrateWebhook, Executor, Webhook};

type BoxedFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;



pub struct Executable<E> {
    prog: Option<String>,
    executor: E,
}

impl<E> Executable<E>
where
    E: Executor + 'static,
{
    pub fn new(prog: Option<String>, executor: E) -> Self {
        Self { prog, executor }
    }
}

impl<E> Executable<E>
where
    E: Executor,
{
    fn do_exec<S: ToString + Send + 'static>(&self, data: Vec<S>) -> BoxedFuture {
        let prog = self.prog.clone();
        Box::pin(async move {
            match prog {
                Some(prog) => {
                    let arg = data
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<String>>();
                    let arg = &arg;

                    let proc = prog
                        .find(":[")
                        .map(|idx| prog.split_at(idx))
                        .map(|(prog, args)| {
                            let args = args
                                .trim_end_matches("]")
                                .trim_start_matches(":[")
                                .split(",");
                            let mut proc = std::process::Command::new(prog);
                            proc.args(args).args(arg);
                            proc
                        })
                        .unwrap_or_else(|| {
                            let mut proc = std::process::Command::new(prog);
                            proc.args(arg);
                            proc
                        })
                        .stdin(std::process::Stdio::null())
                        .output();

                    match proc {
                        Ok(o) => {
                            let fmt = if o.status.success() {
                                String::from_utf8_lossy(&o.stdout)
                            } else {
                                String::from_utf8_lossy(&o.stderr)
                            };

                            log::debug!("{}", fmt);
                        }
                        Err(e) => {
                            log::warn!("notification failed {}", e);
                        }
                    }
                }
                None => {}
            }
        })
    }
}

impl<E> Webhook for Executable<E>
where
    E: Executor,
{
    fn on_error(&self, error: &crate::Error, address: &crate::Address)
    where
        Self: Sized,
    {
        self.executor.spawn(self.do_exec(vec![json!({
            "on": "error",
            "data": {
                "error": error.to_string(),
                "address": address,
            }
        })]));
    }

    fn on_stop(&self, time: std::time::Instant, address: &crate::Address)
    where
        Self: Sized,
    {
        self.executor.spawn(self.do_exec(vec![json!({
            "on": "stop",
            "data": {
                "last": time.elapsed(),
                "from": address
            }
        })]));
    }
}

impl<E> PenetrateWebhook for Executable<E>
where
    E: Executor,
{
    fn on_pen_start(
        &self,
        client: &crate::Address,
        visit: &crate::Address,
        server: &crate::Address,
        config: &crate::penetrate::server::ClientConfig,
    ) where
        Self: Sized,
    {
        self.executor.spawn(self.do_exec(vec![json!({
            "on": "pen_start",
            "data": {
                "client": client,
                "visit": visit,
                "server": server,
                "config": config
            },
        })]));
    }

    fn on_pen_stop(
        &self,
        client: &crate::Address,
        visit: &crate::Address,
        server: &crate::Address,
        config: &crate::penetrate::server::ClientConfig,
    ) where
        Self: Sized,
    {
        self.executor.spawn(self.do_exec(vec![json!({
            "on": "pen_stop",
            "data": {
                "client": client,
                "visit": visit,
                "server": server,
                "config": config
            }
        })]));
    }

    fn on_pen_error(
        &self,
        client: &crate::Address,
        config: &crate::penetrate::server::ClientConfig,
        error: &crate::Error,
    ) where
        Self: Sized,
    {
        self.executor.spawn(self.do_exec(vec![json!({
            "on": "pen_error",
            "data": {
                "client": client,
                "error": error.to_string(),
                "config": config
            }
        })]));
    }
}
