use std::sync::Arc;

use crate::error;

use super::{
    io::{AsyncRead, AsyncWrite},
    BoxedFuture,
};

pub enum Process<A, R> {
    Next(A),
    Reject(A),
    Resolve(R),
}

pub enum Selector<In, Out> {
    Next(In),
    Accepted(Out),
}

pub enum Prepare<In, Out> {
    Ok(Out),
    Bad(In),
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

pub struct AbstractProcessor<'a, A, R>(Box<dyn IProcessor<A, R> + Send + Unpin + 'a>);

pub struct Processor<'a, A, R> {
    processors: Vec<AbstractProcessor<'a, A, R>>,
}

pub struct AbstractPreprocessor<'a, In, Out>(
    pub(crate) Arc<dyn Preprocessor<In, Output = Out> + Sync + Send + 'a>,
);

pub struct PreprocessorSelector<'a, In, Out> {
    pres: Vec<AbstractPreprocessor<'a, In, error::Result<Selector<In, Out>>>>,
}

impl<A, R> IProcessor<A, R> for AbstractProcessor<'_, A, R>
where
    A: Send,
{
    fn process<'a>(&'a mut self, input: A) -> BoxedFuture<'a, Process<A, R>> {
        self.0.process(input)
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
        self.processors.push(AbstractProcessor(Box::new(p)))
    }
}

impl<In, Out> Preprocessor<In> for AbstractPreprocessor<'_, In, Out> {
    type Output = Out;
    fn prepare<'a>(&'a self, input: In) -> BoxedFuture<'a, Self::Output> {
        self.0.prepare(input)
    }
}

impl<In, Out> Clone for AbstractPreprocessor<'_, In, Out> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<'p, In, Out> PreprocessorSelector<'p, In, Out> {
    pub fn new() -> Self {
        Self {
            pres: Default::default(),
        }
    }

    pub fn add<P>(&mut self, p: P)
    where
        P: Preprocessor<In, Output = error::Result<Selector<In, Out>>> + Sync + Send + 'p,
    {
        self.pres.push(AbstractPreprocessor(Arc::new(p)))
    }
}

impl<'p, In, Out> Preprocessor<In> for PreprocessorSelector<'p, In, Out>
where
    In: Send,
{
    type Output = error::Result<Prepare<In, Out>>;

    fn prepare<'a>(&'a self, input: In) -> BoxedFuture<'a, Self::Output> {
        Box::pin(async move {
            let mut input = input;

            for prep in self.pres.iter() {
                input = match prep.prepare(input).await? {
                    Selector::Next(input) => input,
                    Selector::Accepted(out) => return Ok(Prepare::Ok(out)),
                }
            }

            Ok(Prepare::Bad(input))
        })
    }
}
