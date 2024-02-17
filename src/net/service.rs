pub mod error;
use std::net::SocketAddr;
use std::sync::Arc;
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
    Self::LocalData: Send + Sync,
    Self::GlobalData: Send + Sync,
{
    type Packet;
    type Decoder;
    type Encoder;
    type Handler;
    type LocalData;
    type GlobalData;

    fn init(&mut self);

    fn init_global(&mut self) -> Self::GlobalData;

    fn init_local(&self) -> Self::LocalData;

    fn create_handler(&self) -> Self::Handler;

    fn create_decoder(&self) -> Self::Decoder;

    fn create_encoder(&self) -> Self::Encoder;
}

pub trait Handler
where
    Self::Packet: Clone + Send + Sync + 'static,
    Self::Local: Send + Sync,
    Self::Global: Send + Sync,
{
    type Packet;
    type Local;
    type Global;

    fn handle(
        &mut self,
        packet: Self::Packet,
        conn: ConnectionHandle<Self::Packet>,
        local: Arc<Self::Local>,
        global: Arc<Self::Global>,
    ) -> Result<(), Error>;
}

#[async_trait]
pub trait Decoder
where
    Self::Packet: Clone + Send + Sync + 'static,
{
    type Packet;
    async fn decode(
        &self,
        reader: &mut (dyn AsyncRead + Unpin + Send),
    ) -> Result<Self::Packet, Error>;
}

#[async_trait]
pub trait Encoder
where
    Self::Packet: Clone + Send + Sync + 'static,
{
    type Packet;
    async fn encode(
        &self,
        writer: &mut (dyn AsyncWrite + Unpin + Send),
        packet: &Self::Packet,
    ) -> Result<(), Error>;
}

pub struct ConnectionHandle<P> {
    addr: SocketAddr,
    last_time: SystemTime,
    disconnect_fn: Box<dyn Fn()>,
    send_packet_fn: Box<dyn Fn(AddrTarget, P)>,
}

impl<P> ConnectionHandle<P> {
    pub fn new<F, S>(
        addr: SocketAddr,
        last_time: SystemTime,
        disconnect_fn: F,
        send_packet_fn: S,
    ) -> Self
    where
        F: Fn() + 'static,
        S: Fn(AddrTarget, P) + 'static,
    {
        Self {
            addr,
            last_time,
            disconnect_fn: Box::new(disconnect_fn),
            send_packet_fn: Box::new(send_packet_fn),
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

    pub fn send_packet(&self, target: AddrTarget, packet: P) {
        (self.send_packet_fn)(target, packet)
    }
}

#[derive(Debug, Clone)]
pub enum AddrTarget {
    All,
    Only(SocketAddr),
    Without(SocketAddr),
}
