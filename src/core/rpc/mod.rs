use std::{pin::Pin, task::Poll};

use crate::error;

pub trait AsyncCaller {
    fn poll_call(self: Pin<&mut Self>, raw_arg: &[u8]) -> Poll<error::Result<Vec<u8>>>;
}


pub struct Remote{
  
}