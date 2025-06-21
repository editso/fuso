use crate::{
    core::{channel::Sender, future::Poller},
    error,
};

use super::Cmd;

#[derive(Clone)]
pub struct Heartbeat {}

impl Heartbeat {
    pub(super) fn new<'a>(sender: Sender<Cmd>) -> (Poller<'a, error::Result<()>>, Self) {
        (Poller::new(), Self {})
    }

    pub(super) fn pong(&self) {
        
    }

    pub(super) fn ping(&self) {

    }

    pub(super) fn interrupt(&self) {
        
    }
}
