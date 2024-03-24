use std::future::Future;

use crate::error;

pub trait Service {
    fn run(&self) -> error::Result<()>;

    fn name(&self) -> String;
}

pub trait Manager {
    fn manage<S, S1>(&mut self, service: S) -> error::Result<()>
    where
        S: Into<S1>,
        S1: Service + 'static;
}

pub struct MultiServiceManager {}

impl Manager for MultiServiceManager {
    fn manage<S, S1>(&mut self, service: S) -> error::Result<()>
    where
        S: Into<S1>,
        S1: Service + 'static,
    {
        unimplemented!()
    }
}

// impl<T, F> Service for T
// where
//     T: FnOnce() -> F,
//     F: Future<Output = error::Result<()>> + Send,
// {
//     fn run(&self) -> error::Result<()> {
//         todo!()
//     }

//     fn name(&self) -> String {
//         todo!()
//     }
// }
