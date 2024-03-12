pub mod port_forward {
    use serde::{Deserialize, Serialize};

    use crate::{config::client::ServerAddr, core::Connection};

    #[derive(Debug, Serialize, Deserialize)]
    pub enum Request {
        New(u64, Option<ServerAddr>),
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub enum Response {
        Error(),
    }

    pub enum WithSocks {
        Tcp(ServerAddr),
        Udp(),
    }

    pub enum VisitorProtocol {
        Socks(Connection<'static>, WithSocks),
        Other(Connection<'static>, Option<ServerAddr>),
    }
}
