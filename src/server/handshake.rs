use crate::core::{processor::{IProcessor, Process}, BoxedFuture};

pub struct Handshake {
  
}



impl<A, R> IProcessor<A, R> for Handshake {
    fn process<'a>(
        &'a mut self,
        data: A,
    ) -> BoxedFuture<'a, Process<A, R>> {
        Box::pin(async move{
          


          

          

          unimplemented!()
        })
    }
}
