pub mod port_forward {
    use serde::{Deserialize, Serialize};

    use crate::{config::client::ServerAddr, core::Connection};

    #[derive(Debug, Serialize, Deserialize)]
    pub enum Target {
        Udp(ServerAddr, u16),
        Tcp(ServerAddr, u16),
    }
 
    #[derive(Debug, Serialize, Deserialize)]
    pub enum Request {
        New(u64, Option<Target>),
        Dyn(u64, Option<Target>)
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub enum Response {
        Ok,
        Cancel,
        Error(String),
    }

    pub enum VisitorProtocol {
        Socks(Connection<'static>, Target),
        Other(Connection<'static>, Option<Target>),
    }
}
