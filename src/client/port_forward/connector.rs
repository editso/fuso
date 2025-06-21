use rc4::{KeyInit, Rc4, StreamCipher};

use crate::core::{
    connector::{AbstractConnector, Connector},
    stream::handshake::MuxConfig,
    transfer::{AbstractTransmitter, TransmitterExt},
    Stream,
};

use super::Protocol;

pub struct MuxTransmitterConnector<'connector, O> {
    conf: MuxConfig,
    connector: AbstractConnector<'connector, Protocol, O>,
}

pub struct TransmitterConnector<'c, C> {
    connector: AbstractConnector<'c, Protocol, C>,
}

impl<'connector, O> MuxTransmitterConnector<'connector, O> {
    pub fn new<C>(conf: MuxConfig, connector: C) -> Self
    where
        C: Connector<Protocol, Output = O> + Send + Sync + Unpin + 'connector,
    {
        #[cfg(debug_assertions)]
        log::debug!("using mux transmitter connector {:#?}", conf);

        Self {
            conf,
            connector: AbstractConnector::new(connector),
        }
    }
}

impl<'connector, O> Connector<Protocol> for MuxTransmitterConnector<'connector, O>
where
    O: Stream + Send + Unpin + 'static,
{
    type Output = AbstractTransmitter<'connector>;

    fn connect<'conn>(
        &'conn self,
        target: Protocol,
    ) -> crate::core::BoxedFuture<'conn, crate::error::Result<Self::Output>> {
        Box::pin(async move {
            let mut transmitter = self.connector.connect(target).await?;
            let mut magic = self.conf.magic.to_le_bytes();

            Rc4::new((&self.conf.secret).into()).apply_keystream(&mut magic);

            transmitter.send_all(&magic).await?;

            Ok(transmitter.into())
        })
    }
}

impl<'c, O> TransmitterConnector<'c, O> {
    pub fn new<C>(connector: C) -> Self
    where
        C: Connector<Protocol, Output = O> + Send + Sync + Unpin + 'c,
    {
        #[cfg(debug_assertions)]
        log::debug!("using transmitter connector");

        Self {
            connector: AbstractConnector::new(connector),
        }
    }
}

impl<'c, O> Connector<Protocol> for TransmitterConnector<'c, O>
where
    O: Stream + Send + Unpin + 'static,
{
    type Output = AbstractTransmitter<'static>;

    fn connect<'conn>(
        &'conn self,
        target: Protocol,
    ) -> crate::core::BoxedFuture<'conn, crate::error::Result<Self::Output>> {
        Box::pin(async move { self.connector.connect(target).await.map(Into::into) })
    }
}
