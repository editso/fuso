use std::sync::Arc;

use super::{
    io::{AsyncRead, AsyncWrite},
    BoxedFuture,
};

pub enum Process<A, R> {
    Next(A),
    Reject(A),
    Resolve(R),
}

pub trait Preprocessor<In> {
    type Output;

    fn prepare<'a>(&'a self, input: In) -> BoxedFuture<'a, Self::Output>;
}

pub trait IProcessor<A, R> {
    fn process<'a>(&'a mut self, data: A) -> BoxedFuture<'a, Process<A, R>>;
}

pub trait StreamProcessor {
    fn process<'a, P, R>(self, processor: &'a mut P) -> BoxedFuture<'a, Process<Self, R>>
    where
        Self: Sized + Send,
        P: IProcessor<Self, R> + Send + 'a,
    {
        processor.process(self)
    }
}

impl<'a, T> StreamProcessor for T where T: AsyncRead + AsyncWrite {}

pub struct BoxedProcessor<'a, A, R>(Box<dyn IProcessor<A, R> + Send + Unpin + 'a>);

pub struct Processor<'a, A, R> {
    processors: Vec<BoxedProcessor<'a, A, R>>,
}

pub struct BoxedPreprocessor<'a, In, Out>(pub(crate) Arc<dyn Preprocessor<In, Output = Out> + Sync + Send + 'a>);


impl<A, R> IProcessor<A, R> for BoxedProcessor<'_, A, R>
where
    A: Send,
{
    fn process<'a>(&'a mut self, data: A) -> BoxedFuture<'a, Process<A, R>> {
        self.0.process(data)
    }
}

impl<A, R> IProcessor<A, R> for Processor<'_, A, R>
where
    A: Send,
{
    fn process<'a>(&'a mut self, data: A) -> BoxedFuture<'a, Process<A, R>> {
        Box::pin(async move {
            let mut data = data;
            for processor in self.processors.iter_mut() {
                data = match processor.process(data).await {
                    Process::Next(data) => data,
                    Process::Reject(data) => data,
                    Process::Resolve(result) => return Process::Resolve(result),
                }
            }

            Process::Reject(data)
        })
    }
}

impl<'a, A, R> Processor<'a, A, R> {
    pub fn new() -> Self {
        Self {
            processors: Default::default(),
        }
    }

    pub fn add<P>(&mut self, p: P)
    where
        P: IProcessor<A, R> + Unpin + Send + 'a,
    {
        self.processors.push(BoxedProcessor(Box::new(p)))
    }
}


impl<In, Out> Preprocessor<In> for BoxedPreprocessor<'_, In, Out>{
    type Output = Out;
    fn prepare<'a>(&'a self, input: In) -> BoxedFuture<'a, Self::Output> {
        unimplemented!()
    }
}

impl<In, Out> Clone for BoxedPreprocessor<'_, In, Out>{
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
