use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{
    config::{
        client::{PrepareForward, WithForwardService}, AuthWithAccount, AuthWithSecret, Authentication, Compress,
        Crypto, Expose,
    },
    core::{
        protocol::{AsyncPacketRead, AsyncPacketSend},
        rpc::{Decoder, Encoder},
        BoxedFuture, Stream,
    },
    error::{self, FusoError},
    runtime::Runtime,
};

#[derive(Debug, Serialize, Deserialize)]
enum AuthState {
    Ok,
    Reject,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ClientConfig {
    Forward(ForwardConfig),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MuxConfig {
    pub magic: u32,
    pub secret: [u8; 16],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ForwardConfig {
    pub exposes: HashSet<Expose>,
    pub channel: Option<HashSet<Expose>>,
    pub cryptos: HashSet<Crypto>,
    pub compress: HashSet<Compress>,
    pub prepares: PrepareForward,
}

impl<T> Handshake for T where T: Stream + Send + Unpin {}

pub trait Handshake: Stream + Send + Unpin {
    fn client_handshake<'a, R>(
        mut self,
        auth: &'a Authentication,
        timeout: std::time::Duration,
    ) -> BoxedFuture<'a, error::Result<Self>>
    where
        R: Runtime,
        Self: Sized + 'a,
    {
        Box::pin(async move {
            R::wait_for(timeout, async move {
                match auth {
                    Authentication::None => return Ok(self),
                    Authentication::Secret(secret) => {
                        self.send_packet(&secret.encode()?).await?;
                    }
                    Authentication::Account(account) => {
                        self.send_packet(&account.encode()?).await?;
                    }
                }

                let state: AuthState = self.recv_packet().await?.decode()?;

                match state {
                    AuthState::Ok => Ok(self),
                    AuthState::Reject => Err(FusoError::AuthError),
                }
            })
            .await?
        })
    }

    fn server_handshake<'a, R>(
        mut self,
        auth: &'a Authentication,
        timeout: std::time::Duration,
    ) -> BoxedFuture<'a, error::Result<Self>>
    where
        R: Runtime,
        Self: Sized + 'a,
    {
        Box::pin(async move {
            R::wait_for(timeout, async move {
                match auth {
                    Authentication::None => Ok(self),
                    Authentication::Secret(secret) => {
                        log::trace!("using secret auth");

                        let client_secret: AuthWithSecret = self.recv_packet().await?.decode()?;
                        if client_secret.ne(&secret) {
                            self.send_packet(&AuthState::Reject.encode()?).await?;
                            Err(FusoError::AuthError)
                        } else {
                            self.send_packet(&AuthState::Ok.encode()?).await?;
                            Ok(self)
                        }
                    }
                    Authentication::Account(account) => {
                        log::trace!("using account auth");

                        let client_account: AuthWithAccount = self.recv_packet().await?.decode()?;
                        if client_account.ne(&account) {
                            self.send_packet(&AuthState::Reject.encode()?).await?;
                            Err(FusoError::AuthError)
                        } else {
                            self.send_packet(&AuthState::Ok.encode()?).await?;
                            Ok(self)
                        }
                    }
                }
            })
            .await?
        })
    }

    fn read_config<'a>(&'a mut self) -> BoxedFuture<'a, error::Result<ClientConfig>>
    where
        Self: Sized + 'a,
    {
        Box::pin(async move { Ok(self.recv_packet().await?.decode()?) })
    }

    fn write_config<'a, C>(&'a mut self, config: C) -> BoxedFuture<'a, error::Result<()>>
    where
        C: Into<ClientConfig> + Send,
        Self: Sized + 'a,
    {
        let config: ClientConfig = config.into();

        Box::pin(async move {
            log::debug!("send configuration information");
            self.send_packet(&config.encode()?).await
        })
    }
}

impl<'a> From<&'a WithForwardService> for ClientConfig {
    fn from(value: &'a WithForwardService) -> Self {
        Self::Forward(ForwardConfig {
            exposes: value.exposes.clone(),
            prepares: value.prepares.clone(),
            channel: value.channel.clone(),
            cryptos: value.crypto.clone(),
            compress: value.compress.clone(),
        })
    }
}

impl Default for MuxConfig {
    fn default() -> Self {
        Self {
            // @MUX
            magic: 0x404d5558,
            secret: rand::random(),
        }
    }
}
