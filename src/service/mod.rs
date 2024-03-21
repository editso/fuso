use crate::{config::RestartPolicy, core::BoxedFuture, error};

pub trait IService {
    fn call(&self) -> BoxedFuture<'static, error::Result<()>>;
}

pub struct BoxedService {}

pub struct FusoService;

pub struct ServiceManager {}

pub struct ServiceBuilder {}

pub struct NamedService {
    name: String,
}

pub struct FnService(Box<dyn Fn() -> BoxedFuture<'static, error::Result<()>>>);

impl ServiceManager {
    pub fn new() -> ServiceBuilder {
        ServiceBuilder {}
    }
}

impl NamedService {
    pub fn new<N, S>(name: N, service: S) -> Self
    where
        N: ToString,
        S: IService,
    {
        unimplemented!()
    }
}

impl ServiceBuilder {
    pub fn register<S>(&mut self, restart_policy: RestartPolicy, service: S)
    where
        S: IService + 'static,
    {
        unimplemented!()
    }

    pub fn build(self) -> ServiceManager {
        unimplemented!()
    }
}

impl std::future::Future for ServiceManager {
    type Output = error::Result<()>;
    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        unimplemented!()
    }
}

impl IService for NamedService {
    fn call(&self) -> BoxedFuture<'static, error::Result<()>> {
        unimplemented!()
    }
}

impl IService for FnService {
    fn call(&self) -> BoxedFuture<'static, error::Result<()>> {
        unimplemented!()
    }
}

impl FnService {
    pub fn new<F, Fut>(f: F) -> Self
    where
        F: Fn() -> Fut + 'static,
        Fut: std::future::Future<Output = error::Result<()>> + Send + 'static,
    {
        Self(Box::new(move || Box::pin(f())))
    }
}
