pub mod error;
pub mod relay;

use crate::data::packet::Packet;

use self::error::HandlePacketError;

use super::connection::Connection;

use async_trait::async_trait;

pub trait Service {
    fn create_handler(&self, conn: Connection) -> Box<dyn ServiceHandler + Send + Sync>;
}

pub struct NeverService;

impl Service for NeverService {
    fn create_handler(&self, _conn: Connection) -> Box<dyn ServiceHandler + Send + Sync> {
        panic!("never service!")
    }
}

#[async_trait]
pub trait ServiceHandler {
    async fn read_packet(&mut self) -> Result<Packet, std::io::Error>;
    async fn handle_packet(&mut self, packet: &Packet) -> Result<(), HandlePacketError>;
}
