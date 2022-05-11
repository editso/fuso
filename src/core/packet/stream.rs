use std::future::Future;

use crate::{AsyncRead, AsyncWrite, Result};

use super::Packet;

pub struct RecvPacket<'a, R>
where
    R: Unpin,
{
    reader: &'a mut R,
}

pub struct SendPacket<'a, W>
where
    W: Unpin,
{
    packet: Vec<u8>,
    writer: &'a mut W,
}

pub trait AsyncRecvPacket: AsyncRead {
    fn recv_packet<'a>(&'a mut self) -> RecvPacket<'a, Self>
    where
        Self: Unpin + Sized,
    {
        RecvPacket { reader: self }
    }
}

pub trait AsyncSendPacket: AsyncWrite {
    fn send_packet<'a>(&'a mut self, packet: &[u8]) -> SendPacket<'a, Self>
    where
        Self: Unpin + Sized,
    {
        SendPacket {
            packet: packet.to_vec(),
            writer: self,
        }
    }
}

impl<'a, T> Future for RecvPacket<'a, T>
where
    T: AsyncRead + Unpin,
{
    type Output = Result<Packet>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        todo!()
    }
}

impl<'a, T> Future for SendPacket<'a, T>
where
    T: AsyncWrite + Unpin,
{
    type Output = Result<()>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        unimplemented!()
    }
}

impl<T> AsyncSendPacket for T where T: AsyncWrite + Unpin {}

impl<T> AsyncRecvPacket for T where T: AsyncRead + Unpin {}
