use std::{marker::PhantomData, pin::Pin, task::Poll};

use crate::{
    config::RestartPolicy,
    core::{future::Poller, BoxedFuture},
    error,
    runtime::Runtime,
};

pub trait IRunnable: Sync {
    fn call(&self) -> BoxedFuture<'static, error::Result<Rise>>;

    fn name<'a>(&'a self) -> &'a str {
        "Unknown"
    }

    fn restartable(&mut self) -> bool {
        false
    }
}

pub struct BoxedRunnable<'a>(Box<dyn IRunnable + Send + 'a>);

pub enum Rise {
    Exit,
    Fatal,
    Restart,
}

pub struct ServiceRunner<'a, Runtime> {
    poller: Poller<'a, (BoxedRunnable<'a>, error::Result<Rise>)>,
    _marked: PhantomData<Runtime>,
}

pub struct RunnerBuilder<'a> {
    poller: Poller<'a, (BoxedRunnable<'a>, error::Result<Rise>)>,
}

pub struct AlwayRestartRunnable<R> {
    runnable: R,
}

pub struct OnceRestartRunnable<R> {
    runnable: R,
}

pub struct CounterRestartRunnable<R> {
    runnable: R,
    counter: usize,
}

pub struct NamedRunnable<R> {
    name: String,
    runnable: R,
}

enum Runnable<'a> {
    Running,
    Pause(BoxedRunnable<'a>, BoxedFuture<'a, error::Result<Rise>>),
}

pub struct FnRunnable(Box<dyn Fn() -> BoxedFuture<'static, error::Result<Rise>> + Send>);

impl<'a> BoxedRunnable<'a> {
    pub fn new<S>(service: S) -> Self
    where
        S: IRunnable + Send + 'a,
    {
        Self(Box::new(service))
    }
}

impl<'a, Runtime> ServiceRunner<'a, Runtime> {
    pub fn new() -> RunnerBuilder<'a> {
        RunnerBuilder {
            poller: Poller::new(),
        }
    }
}

impl<'a> RunnerBuilder<'a> {
    pub fn register<S>(&mut self, restart_policy: RestartPolicy, runnable: S)
    where
        S: IRunnable + Send + 'static,
    {
        self.poller.add({
            let fut = runnable.call();
            Runnable::Pause(
                {
                    match restart_policy {
                        RestartPolicy::Never => {
                            BoxedRunnable::new(OnceRestartRunnable { runnable })
                        }
                        RestartPolicy::Always => {
                            BoxedRunnable::new(AlwayRestartRunnable { runnable })
                        }
                        RestartPolicy::Counter => BoxedRunnable::new(CounterRestartRunnable {
                            runnable,
                            counter: 1,
                        }),
                    }
                },
                fut,
            )
        })
    }

    pub fn build<Runtime>(self) -> ServiceRunner<'a, Runtime> {
        ServiceRunner {
            poller: self.poller,
            _marked: PhantomData,
        }
    }
}

impl<R> std::future::Future for ServiceRunner<'_, R>
where
    R: Runtime + Unpin,
{
    type Output = error::Result<()>;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        loop {
            match Pin::new(&mut self.poller).poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready((mut runnable, result)) => {
                    log::debug!("{} stooped", runnable.name());
                    match runnable.restartable() {
                        true => match result {
                            Ok(Rise::Fatal | Rise::Exit) => {}
                            _ => {
                                let fut = runnable.call();
                                log::debug!("restart {}", runnable.name());
                                self.poller.add(Runnable::Pause(runnable, {
                                    Box::pin(async move {
                                        R::sleep(std::time::Duration::from_secs(1)).await;
                                        fut.await
                                    })
                                }))
                            }
                        },
                        false => {
                            log::debug!("finished task")
                        }
                    };
                }
            }
        }
    }
}

impl FnRunnable {
    pub fn new<F, Fut>(f: F) -> Self
    where
        F: Fn() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = error::Result<Rise>> + Send + 'static,
    {
        Self(Box::new(move || Box::pin(f())))
    }
}

impl<R> NamedRunnable<R>
where
    R: IRunnable,
{
    pub fn new<N>(name: N, runnable: R) -> Self
    where
        N: ToString,
    {
        Self {
            name: name.to_string(),
            runnable,
        }
    }
}

unsafe impl Sync for FnRunnable {}

impl IRunnable for FnRunnable {
    fn call(&self) -> BoxedFuture<'static, error::Result<Rise>> {
        (self.0)()
    }
}

impl IRunnable for BoxedRunnable<'_> {
    fn call(&self) -> BoxedFuture<'static, error::Result<Rise>> {
        self.0.call()
    }

    fn name<'a>(&'a self) -> &'a str {
        self.0.name()
    }

    fn restartable(&mut self) -> bool {
        self.0.restartable()
    }
}

impl<R> IRunnable for NamedRunnable<R>
where
    R: IRunnable,
{
    fn call(&self) -> BoxedFuture<'static, error::Result<Rise>> {
        self.runnable.call()
    }

    fn name<'a>(&'a self) -> &'a str {
        &self.name
    }
}

impl<R> IRunnable for OnceRestartRunnable<R>
where
    R: IRunnable,
{
    fn call(&self) -> BoxedFuture<'static, error::Result<Rise>> {
        self.runnable.call()
    }

    fn name<'a>(&'a self) -> &'a str {
        self.runnable.name()
    }

    fn restartable(&mut self) -> bool {
        false
    }
}

impl<R> IRunnable for CounterRestartRunnable<R>
where
    R: IRunnable,
{
    fn call(&self) -> BoxedFuture<'static, error::Result<Rise>> {
        self.runnable.call()
    }

    fn name<'a>(&'a self) -> &'a str {
        self.runnable.name()
    }

    fn restartable(&mut self) -> bool {
        if self.counter > 0 {
            self.counter -= 1;
        }

        self.counter > 0
    }
}

impl<R> IRunnable for AlwayRestartRunnable<R>
where
    R: IRunnable,
{
    fn call(&self) -> BoxedFuture<'static, error::Result<Rise>> {
        self.runnable.call()
    }

    fn name<'a>(&'a self) -> &'a str {
        self.runnable.name()
    }

    fn restartable(&mut self) -> bool {
        true
    }
}

impl<'a> std::future::Future for Runnable<'a> {
    type Output = (BoxedRunnable<'a>, error::Result<Rise>);
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Self::Output> {
        match std::mem::replace(&mut *self, Runnable::Running) {
            Runnable::Running => unsafe { std::hint::unreachable_unchecked() },
            Runnable::Pause(runnable, mut fut) => match Pin::new(&mut fut).poll(cx) {
                Poll::Ready(r) => Poll::Ready((runnable, r)),
                Poll::Pending => {
                    drop(std::mem::replace(
                        &mut *self,
                        Runnable::Pause(runnable, fut),
                    ));
                    Poll::Pending
                }
            },
        }
    }
}
