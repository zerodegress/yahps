use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    net::SocketAddr,
    sync::{atomic::AtomicBool, Arc, Weak},
};

use async_trait::async_trait;
use dashmap::DashMap;
use tokio::sync::RwLock;

use crate::{
    data::packet::{Packet, PacketBuilder, PacketReader, PacketType},
    net::{connection::Connection, service::error::HandlePacketErrorKind},
};

use super::{HandlePacketError, Service, ServiceHandler};

pub struct RelayService {}

impl Service for RelayService {
    fn create_handler(&self, conn: Connection) -> Box<dyn ServiceHandler + Send + Sync> {
        todo!()
    }
}

pub struct RelayServiceHandler {
    conn: Arc<RelayConnection>,
}

#[async_trait]
impl ServiceHandler for RelayServiceHandler {
    async fn read_packet(&mut self) -> Result<Packet, std::io::Error> {
        self.conn.recv_packet().await
    }

    async fn handle_packet(&mut self, packet: &Packet) -> Result<(), HandlePacketError> {
        todo!()
    }
}

impl RelayServiceHandler {
    async fn relay_check(&mut self, packet: &Packet) -> Result<bool, HandlePacketError> {
        match *self.conn.permission_status.read().await {
            RelayPermissionStatus::PlayerPermission
            | RelayPermissionStatus::PlayerJoinPermission
            | RelayPermissionStatus::HostPermission
            | RelayPermissionStatus::Debug => Ok(false),
            RelayPermissionStatus::InitialConnection => {
                if let PacketType::PREREGISTER_INFO_RECEIVE = packet.ty() {
                    let mut permission_status_writer = self.conn.permission_status.write().await;
                    *permission_status_writer = RelayPermissionStatus::GetPlayerInfo;
                    self.conn
                        .send_packet(
                            PacketBuilder::default()
                                .with_type(PacketType::PREREGISTER_INFO)
                                .write_str("net.rwhps.server.relayGetUUIDHex.Dr")
                                .write_i32(1)
                                .write_i32(0)
                                .write_str("com.corrodinggames.rts.server")
                                .write_str("RCN Team & Tiexiu.xyz Core Team")
                                .write_i32({
                                    let mut hasher = DefaultHasher::new();
                                    "Dr @ 2022".hash(&mut hasher);
                                    hasher.finish() as i32
                                })
                                .build(),
                        )
                        .await
                        .map_err(|err| {
                            HandlePacketError::new(HandlePacketErrorKind::Disconnect, err)
                        })?;
                    Ok(true)
                } else {
                    todo!()
                }
            }
            RelayPermissionStatus::GetPlayerInfo => {
                todo!()
            }
            RelayPermissionStatus::WaitCertified => {
                todo!()
            }
            RelayPermissionStatus::CertifiedEnd => {
                todo!()
            }
        }
    }
}

struct RelayConnection {
    last_receive_time: u64,
    addr: SocketAddr,
    conn: Connection,
    num_of_retries: u64,
    permission_status: RwLock<RelayPermissionStatus>,
    register_player_id: Option<String>,
    name: String,
    relay_room: Option<Arc<RelayRoom>>,
}

impl RelayConnection {
    pub fn addr(&self) -> &SocketAddr {
        &self.addr
    }

    pub fn num_of_retries(&self) -> &u64 {
        &self.num_of_retries
    }

    pub async fn permission_status(&self) -> RelayPermissionStatus {
        *self.permission_status.read().await
    }

    pub async fn recv_packet(&self) -> Result<Packet, std::io::Error> {
        self.conn.read_packet().await
    }

    pub async fn send_packet(&self, packet: Packet) -> Result<(), std::io::Error> {
        self.conn.send_packet(packet).await
    }

    pub async fn relay_register_connection(
        &mut self,
        packet: Packet,
    ) -> Result<(), std::io::Error> {
        let permission_status = self.permission_status().await;
        if self.register_player_id.is_none() {
            let mut reader: PacketReader = packet.into();
            reader.read_str()?;
            reader.skip(12)?;
            self.name = reader.read_str()?;
            reader.read_is_str()?;
            reader.read_str()?;
            self.register_player_id = Some(reader.read_str()?);
            Ok(())
        } else if let RelayPermissionStatus::PlayerPermission
        | RelayPermissionStatus::PlayerJoinPermission
        | RelayPermissionStatus::HostPermission
        | RelayPermissionStatus::Debug = permission_status
        {
            if let RelayPermissionStatus::PlayerPermission = permission_status {
                if let Some(relay_room) = self.relay_room.as_ref() {
                    todo!()
                }
                todo!()
            }
            todo!()
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RelayPermissionStatus {
    InitialConnection,
    GetPlayerInfo,
    WaitCertified,
    CertifiedEnd,
    PlayerPermission,
    PlayerJoinPermission,
    HostPermission,
    Debug,
}

pub struct RelayRoom {
    is_start_game: AtomicBool,
    conn_map: DashMap<i32, Weak<RelayConnection>>,
}

pub struct PlayerRelay {
    conn: Weak<RelayConnection>,
    uuid: String,
    name: String,
}
