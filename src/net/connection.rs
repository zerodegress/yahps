use std::{io::Write, net::SocketAddr};

use bytes::{BufMut, Bytes};
use futures::TryFutureExt;
use tokio::{
    net::tcp::OwnedReadHalf,
    sync::{mpsc, RwLock},
};

use crate::data::packet::Packet;

pub struct Connection {
    addr: SocketAddr,
    stream_reader: RwLock<OwnedReadHalf>,
    packet_tx: mpsc::Sender<Packet>,
}

impl Connection {
    pub fn connect(
        addr: SocketAddr,
        stream_reader: OwnedReadHalf,
        packet_tx: mpsc::Sender<Packet>,
    ) -> Self {
        Self {
            addr,
            stream_reader: RwLock::new(stream_reader),
            packet_tx,
        }
    }

    pub async fn read_packet(&self) -> Result<Packet, std::io::Error> {
        Packet::read_from_net(&mut *self.stream_reader.write().await).await
    }

    pub async fn send_packet(&self, packet: Packet) -> Result<(), std::io::Error> {
        self.packet_tx
            .send(packet)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))
            .await
    }
}
