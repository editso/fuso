use fuso::{
    cli,
    config::{
        client::{Config, Service, WithBridgeService, WithForwardService, WithProxyService},
        Stateful,
    },
    error,
    service::{FnService, NamedService, ServiceManager},
};

fn main() {
    match cli::client::parse() {
        Ok(conf) => fuso::enter_async_main(enter_fuso_main(conf)).unwrap(),
        Err(e) => {
            log::error!("{e:?}")
        }
    }
}

fn enter_fuso_main(mut conf: Config) -> ServiceManager {
    let mut sp = ServiceManager::new();

    let services = std::mem::replace(&mut conf.services, Default::default());

    let conf = Stateful::new(conf);

    for (name, service) in services {
        let sc = conf.clone();
        match service {
            Service::Proxy(s) => {
                sp.register(
                    s.restart.clone(),
                    NamedService::new(name, {
                        FnService::new(move || enter_proxy_service_main(sc.clone(), s.clone()))
                    }),
                );
            }
            Service::Bridge(s) => {
                sp.register(
                    s.restart.clone(),
                    NamedService::new(name, {
                        FnService::new(move || enter_bridge_service_main(sc.clone(), s.clone()))
                    }),
                );
            }
            Service::Forward(s) => {
                sp.register(
                    s.restart.clone(),
                    NamedService::new(name, {
                        FnService::new(move || enter_forward_service_main(sc.clone(), s.clone()))
                    }),
                );
            }
        }
    }

    sp.build()
}

async fn enter_proxy_service_main(
    config: Stateful<Config>,
    service: WithProxyService,
) -> error::Result<()> {
    Ok(())
}

async fn enter_bridge_service_main(
    config: Stateful<Config>,
    service: WithBridgeService,
) -> error::Result<()> {
    Ok(())
}

async fn enter_forward_service_main(
    config: Stateful<Config>,
    service: WithForwardService,
) -> error::Result<()> {
    Ok(())
}
