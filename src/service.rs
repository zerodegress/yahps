pub mod error;
use std::hash::Hash;
use std::sync::Arc;
use std::time::SystemTime;

pub use self::error::Error;
pub use self::error::ErrorKind;
use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};

pub trait Service: Send + Sync
where
    Self::Packet: Clone + Send + Sync + 'static,
    Self::Decoder: Decoder<Packet = Self::Packet> + Send + Sync,
    Self::Encoder: Encoder<Packet = Self::Packet> + Send + Sync,
    Self::Handler: Handler<Packet = Self::Packet, Addr = Self::Addr> + Send + Sync,
    Self::Addr: Clone + Eq + PartialEq + Hash + Send + Sync,
    Self::GlobalConnectionHandle: GlobalConnectionHandle<Packet = Self::Packet, Addr = Self::Addr>,
{
    type Packet;
    type Decoder;
    type Encoder;
    type Handler;
    type Addr;
    type GlobalConnectionHandle;

    fn init(&mut self, global_conn: Self::GlobalConnectionHandle);

    fn create_handler(&self) -> Self::Handler;

    fn create_decoder(&self) -> Self::Decoder;

    fn create_encoder(&self) -> Self::Encoder;
}

pub trait Handler
where
    Self::Packet: Clone + Send + Sync + 'static,
    Self::Local: Default + Send + Sync,
    Self::Addr: Clone + Eq + PartialEq + Hash + Send + Sync,
    Self::ConnectionHandle: ConnectionHandle<Packet = Self::Packet, Addr = Self::Addr>,
{
    type ConnectionHandle;
    type Packet;
    type Local;
    type Addr;

    fn handle(
        &mut self,
        packet: Self::Packet,
        conn: Self::ConnectionHandle,
        local: Arc<Self::Local>,
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

pub trait GlobalConnectionHandle: Default
where
    Self::Packet: Clone + Send + Sync + 'static,
    Self::Addr: Clone + Eq + PartialEq + Hash + Send + Sync,
{
    type Packet;
    type Addr;

    fn disconnect(&self, target: AddrTarget<Self::Addr>);
    fn send_packet(&self, target: AddrTarget<Self::Addr>, packet: Self::Packet);
}

#[derive(Clone)]
pub struct DefaultGlobalConnectionHandle<
    P: Clone + Send + Sync + 'static,
    A: Clone + Eq + PartialEq + Hash + Send + Sync,
> {
    disconnect_fn: Arc<dyn Fn(AddrTarget<A>) + Send + Sync>,
    send_packet_fn: Arc<dyn Fn(AddrTarget<A>, P) + Send + Sync>,
}

impl<P: Clone + Send + Sync + 'static, A: Clone + Eq + PartialEq + Hash + Send + Sync> Default
    for DefaultGlobalConnectionHandle<P, A>
{
    fn default() -> Self {
        Self {
            disconnect_fn: Arc::new(|_| {}),
            send_packet_fn: Arc::new(|_, _| {}),
        }
    }
}

impl<P: Clone + Send + Sync + 'static, A: Clone + Eq + PartialEq + Hash + Send + Sync>
    DefaultGlobalConnectionHandle<P, A>
{
    pub fn new(
        disconnect_fn: impl Fn(AddrTarget<A>) + Send + Sync + 'static,
        send_packet_fn: impl Fn(AddrTarget<A>, P) + Send + Sync + 'static,
    ) -> Self {
        Self {
            disconnect_fn: Arc::new(disconnect_fn),
            send_packet_fn: Arc::new(send_packet_fn),
        }
    }
}

impl<P: Clone + Send + Sync + 'static, A: Clone + Eq + PartialEq + Hash + Send + Sync>
    GlobalConnectionHandle for DefaultGlobalConnectionHandle<P, A>
{
    type Packet = P;
    type Addr = A;

    fn disconnect(&self, target: AddrTarget<A>) {
        (self.disconnect_fn)(target)
    }

    fn send_packet(&self, target: AddrTarget<A>, packet: P) {
        (self.send_packet_fn)(target, packet)
    }
}

pub trait ConnectionHandle
where
    Self::Packet: Clone + Send + Sync + 'static,
    Self::Addr: Clone + Eq + PartialEq + Hash + Send + Sync,
{
    type Packet;
    type Addr;

    fn addr(&self) -> &Self::Addr;

    fn last_time(&self) -> &SystemTime;

    fn disconnect(&self);

    fn send_packet(&self, packet: Self::Packet);
}

#[derive(Clone)]
pub struct DefaultConnectionHandle<
    P: Clone + Send + Sync + 'static,
    A: Clone + Eq + PartialEq + Hash + Send + Sync,
> {
    addr: A,
    last_time: SystemTime,
    disconnect_fn: Arc<dyn Fn()>,
    send_packet_fn: Arc<dyn Fn(P)>,
}

impl<P: Clone + Send + Sync + 'static, A: Clone + Eq + PartialEq + Hash + Send + Sync>
    DefaultConnectionHandle<P, A>
{
    pub fn new<F, S>(addr: A, last_time: SystemTime, disconnect_fn: F, send_packet_fn: S) -> Self
    where
        F: Fn() + 'static,
        S: Fn(P) + 'static,
    {
        Self {
            addr,
            last_time,
            disconnect_fn: Arc::new(disconnect_fn),
            send_packet_fn: Arc::new(send_packet_fn),
        }
    }
}

impl<P: Clone + Send + Sync + 'static, A: Clone + Eq + PartialEq + Hash + Send + Sync>
    ConnectionHandle for DefaultConnectionHandle<P, A>
{
    type Packet = P;
    type Addr = A;

    fn addr(&self) -> &A {
        &self.addr
    }

    fn last_time(&self) -> &SystemTime {
        &self.last_time
    }

    fn disconnect(&self) {
        (self.disconnect_fn)()
    }

    fn send_packet(&self, packet: P) {
        (self.send_packet_fn)(packet)
    }
}

#[derive(Debug, Clone)]
pub enum AddrTarget<Addr: Clone + Eq + PartialEq + Hash + Send + Sync> {
    All,
    Only(Addr),
    Without(Addr),
}
