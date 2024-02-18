use std::{net::SocketAddr, str::FromStr};

use clap::Parser;
use service::ReverseProxy;
use yahps::net::server::Server;

pub mod packet;
pub mod service;

#[derive(clap::Parser, Debug)]
struct CliArgs {
    #[arg(long)]
    server: String,
}

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    env_logger::init();
    let args = CliArgs::parse();
    Server::new(ReverseProxy::new(
        SocketAddr::from_str(&args.server).unwrap(),
    ))
    .bind(([127, 0, 0, 1], 5124))
    .run()
    .await
}
