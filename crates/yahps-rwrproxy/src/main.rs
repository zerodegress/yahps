use service::Proxy;
use yahps::net::server::Server;

pub mod packet;
pub mod service;

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    env_logger::init();
    Server::new(Proxy).bind(([127, 0, 0, 1], 5123)).run().await
}
