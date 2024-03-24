use crate::{
    config::{AuthWithAccount, AuthWithSecret, Authentication},
    core::{
        protocol::{AsyncPacketRead, AsyncPacketSend},
        rpc::{Decoder, Encoder},
        BoxedFuture, Stream,
    },
    error,
};

pub trait Handshake: Stream + Send + Unpin {
    fn client_handshake<'a>(
        mut self,
        auth: &'a Authentication,
    ) -> BoxedFuture<'a, error::Result<Self>>
    where
        Self: Sized + 'a,
    {
        Box::pin(async move {
            match auth {
                Authentication::None => Ok(self),
                Authentication::Secret(secret) => {
                    self.send_packet(&secret.encode()?).await?;
                    let token: usize = self.recv_packet().await?.decode()?;
                    unimplemented!()
                }
                Authentication::Account(account) => {
                    self.send_packet(&account.encode()?).await?;
                    let token: usize = self.recv_packet().await?.decode()?;
                    unimplemented!()
                }
            }
        })
    }

    fn server_handshake<'a>(
        mut self,
        auth: &'a Authentication,
    ) -> BoxedFuture<'a, error::Result<Self>>
    where
        Self: Sized + 'a,
    {
        Box::pin(async move {
            match auth {
                Authentication::None => Ok(self),
                Authentication::Secret(secret) => {
                    let client_secret: AuthWithSecret = self.recv_packet().await?.decode()?;
                    unimplemented!()
                }
                Authentication::Account(account) => {
                    let client_account: AuthWithAccount = self.recv_packet().await?.decode()?;
                    unimplemented!()
                }
            }
        })
    }
}

impl<T> Handshake for T where T: Stream + Send + Unpin {}
