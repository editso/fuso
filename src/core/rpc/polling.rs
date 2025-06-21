use std::{future::Future, pin::Pin};

use crate::{
    core::{
        channel::{self, Receiver, Sender},
        future::Select,
        protocol::{AsyncPacketRead, AsyncPacketSend},
        split::{ReadHalf, WriteHalf},
        Stream,
    },
    error,
};

use super::{keep::Heartbeat, Cmd, Decoder, Encoder, Transact};
use crate::core::split::SplitStream;

pub struct Looper<'looper>(Select<'looper, error::Result<()>>);

impl<'looper> Looper<'looper> {
    pub(super) fn new<S>(stream: S) -> (Self, Sender<Cmd>, Receiver<Transact>)
    where
        S: Stream + Unpin + Send + 'looper,
    {
        let (prx, pax) = channel::open();
        let (psrx, psax) = channel::open();

        let (poller, heartbeat) = Heartbeat::new(prx.clone());

        let (reader, writer) = stream.split();

        let mut select = Select::new();

        select.add(poller);
        select.add(Self::run_recv_loop(writer, pax));
        select.add(Self::run_send_loop(reader, psrx, heartbeat));

        (Self(select), prx, psax)
    }

    pub(super) fn post<F>(&mut self, f: F)
    where
        F: Future<Output = error::Result<()>> + Send + 'looper,
    {
        self.0.add(f);
    }
}

impl<'a> std::future::Future for Looper<'a> {
    type Output = error::Result<()>;
    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}

impl<'looper> Looper<'looper> {
    pub(super) async fn run_send_loop<S>(
        reader: ReadHalf<S>,
        sender: Sender<Transact>,
        heartbeat: Heartbeat,
    ) -> error::Result<()>
    where
        S: Stream + Unpin,
    {
        let mut reader = reader;

        loop {
            let data = reader.recv_packet().await?;
            let cmd: Cmd = data.decode()?;
            match cmd {
                Cmd::Pong => heartbeat.pong(),
                Cmd::Ping => heartbeat.ping(),
                Cmd::Cancel(token) => {
                    sender.send(Transact::Cancel(token)).await?;
                }
                Cmd::Transact { token, packet } => {
                    heartbeat.interrupt();
                    sender.send(Transact::Request(token, packet)).await?;
                }
            };
        }
    }

    pub(super) async fn run_recv_loop<S>(
        writer: WriteHalf<S>,
        receiver: Receiver<Cmd>,
    ) -> error::Result<()>
    where
        S: Stream + Unpin,
    {
        let mut writer = writer;
        loop {
            let pkt: Vec<u8> = receiver.recv().await?.encode()?;
            log::trace!("send to transport {}bytes", pkt.len());
            writer.send_packet(&pkt).await?;
        }
    }
}
