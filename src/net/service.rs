pub mod error;

use std::hash::BuildHasher;
use std::hash::Hash;
use std::net::SocketAddr;
use std::time::SystemTime;

pub use self::error::Error;
pub use self::error::ErrorKind;

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};

pub trait Service: Send + Sync + 'static
where
    Self::Packet: Clone + Send + Sync + 'static,
    Self::Decoder: Decoder<Packet = Self::Packet> + Send + Sync,
    Self::Encoder: Encoder<Packet = Self::Packet> + Send + Sync,
    Self::Handler: Handler<Packet = Self::Packet, Local = Self::LocalData, Global = Self::GlobalData>
        + Send
        + Sync,
    Self::LocalData: Clone,
    Self::GlobalData: Clone,
{
    type Packet;
    type Decoder;
    type Encoder;
    type Handler;
    type LocalData;
    type GlobalData;

    fn create_handler(&self) -> Self::Handler;

    fn create_decoder(&self) -> Self::Decoder;

    fn create_encoder(&self) -> Self::Encoder;
}

pub trait Handler
where
    Self::Packet: Clone + Send + Sync + 'static,
    Self::Local: Clone,
    Self::Global: Clone,
{
    type Packet;
    type Local;
    type Global;

    fn handle(
        &mut self,
        packet: Self::Packet,
        conn: ConnectionHandle,
        local: Option<Self::Local>,
        global: Option<Self::Global>,
    ) -> Result<(), Error>;
}

#[async_trait]
pub trait Decoder
where
    Self::Packet: Clone + Send + Sync + 'static,
{
    type Packet;
    async fn decode(&self, reader: &mut dyn AsyncRead) -> Result<Self::Packet, Error>;
}

#[async_trait]
pub trait Encoder
where
    Self::Packet: Clone + Send + Sync + 'static,
{
    type Packet;
    async fn encode(&self, writer: &mut dyn AsyncWrite, packet: &Self::Packet)
        -> Result<(), Error>;
}

pub struct ConnectionHandle {
    addr: SocketAddr,
    last_time: SystemTime,
    disconnect_fn: Box<dyn Fn()>,
}

impl ConnectionHandle {
    pub fn new<F>(addr: SocketAddr, last_time: SystemTime, disconnect_fn: F) -> Self
    where
        F: Fn() + 'static,
    {
        Self {
            addr,
            last_time,
            disconnect_fn: Box::new(disconnect_fn),
        }
    }

    pub fn addr(&self) -> &SocketAddr {
        &self.addr
    }

    pub fn last_time(&self) -> &SystemTime {
        &self.last_time
    }

    pub fn disconnect(&self) {
        (self.disconnect_fn)()
    }
}
