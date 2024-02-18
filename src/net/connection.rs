use std::net::SocketAddr;

use crate::service;

pub type ConnectionHandle<P> = service::DefaultConnectionHandle<P, SocketAddr>;
pub type GlobalConnectionHandle<P> = service::DefaultGlobalConnectionHandle<P, SocketAddr>;
