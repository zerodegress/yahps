use std::net::SocketAddr;

use async_trait::async_trait;
use log::info;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::packet::Packet;

use yahps::service::{self, Service};

pub struct Proxy;

impl Service for Proxy {
    type Packet = Packet;
    type Handler = Handler;
    type Decoder = Decoder;
    type Encoder = Encoder;
    type LocalData = ();
    type Addr = SocketAddr;

    fn init(&mut self) {}

    fn init_local(&self) -> Self::LocalData {}

    fn create_decoder(&self) -> Self::Decoder {
        Decoder
    }

    fn create_encoder(&self) -> Self::Encoder {
        Encoder
    }

    fn create_handler(&self) -> Self::Handler {
        Handler
    }
}

pub struct Decoder;

#[async_trait]
impl service::Decoder for Decoder {
    type Packet = Packet;
    async fn decode(
        &self,
        reader: &mut (dyn AsyncRead + Unpin + Send),
    ) -> Result<Self::Packet, service::Error> {
        self._decode(reader).await
    }
}

impl Decoder {
    async fn _decode(
        &self,
        reader: &mut (dyn AsyncRead + Unpin + Send),
    ) -> Result<Packet, service::Error> {
        Packet::read_from_net(&mut Box::new(reader))
            .await
            .map_err(|err| {
                service::Error::new(
                    match err.kind() {
                        std::io::ErrorKind::ConnectionReset => service::ErrorKind::Disconnect,
                        _ => service::ErrorKind::WarnOnly,
                    },
                    err,
                )
            })
    }
}

pub struct Encoder;

#[async_trait]
impl service::Encoder for Encoder {
    type Packet = Packet;
    async fn encode(
        &self,
        writer: &mut (dyn AsyncWrite + Unpin + Send),
        packet: &Self::Packet,
    ) -> Result<(), service::Error> {
        packet
            .write_into_net(&mut Box::new(writer))
            .await
            .map_err(|err| service::Error::new(service::ErrorKind::WarnOnly, err))
    }
}

pub struct Handler;

impl service::Handler for Handler {
    type Packet = Packet;
    type Local = ();
    type Addr = SocketAddr;

    fn handle(
        &mut self,
        packet: Self::Packet,
        _conn: service::ConnectionHandle<Self::Packet, Self::Addr>,
        _local: std::sync::Arc<Self::Local>,
    ) -> Result<(), service::Error> {
        info!("{:?}", packet.ty());
        // conn.disconnect();
        Ok(())
    }
}
