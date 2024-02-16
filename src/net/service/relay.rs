use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    net::SocketAddr,
    sync::{atomic::AtomicBool, Arc, Weak},
};

use async_trait::async_trait;
use dashmap::DashMap;
use parking_lot::RwLock;

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
        match self.conn.permission_status() {
            RelayPermissionStatus::PlayerPermission
            | RelayPermissionStatus::PlayerJoinPermission
            | RelayPermissionStatus::HostPermission
            | RelayPermissionStatus::Debug => Ok(false),
            RelayPermissionStatus::InitialConnection => {
                if let PacketType::PREREGISTER_INFO_RECEIVE = packet.ty() {
                    {
                        let mut permission_status = self.conn.permission_status.write();
                        *permission_status = RelayPermissionStatus::GetPlayerInfo;
                    }
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
    register_player_id: RwLock<Option<String>>,
    name: RwLock<String>,
    relay_room: Option<Arc<RelayRoom>>,
    player_relay: RwLock<Option<Arc<PlayerRelay>>>,
}

impl RelayConnection {
    pub fn addr(&self) -> &SocketAddr {
        &self.addr
    }

    pub fn num_of_retries(&self) -> &u64 {
        &self.num_of_retries
    }

    pub fn permission_status(&self) -> RelayPermissionStatus {
        *self.permission_status.read()
    }

    pub async fn recv_packet(&self) -> Result<Packet, std::io::Error> {
        self.conn.read_packet().await
    }

    pub async fn send_packet(&self, packet: Packet) -> Result<(), std::io::Error> {
        self.conn.send_packet(packet).await
    }

    pub async fn relay_register_connection(
        self: &Arc<Self>,
        packet: Packet,
    ) -> Result<(), HandlePacketError> {
        let permission_status = self.permission_status();
        let register_player_id = self.register_player_id.read().clone();
        if register_player_id.is_none() {
            let mut reader: PacketReader = packet.clone().into();
            reader
                .read_str()
                .map_err(|err| HandlePacketError::new(HandlePacketErrorKind::Ignorable, err))?;
            reader
                .skip(12)
                .map_err(|err| HandlePacketError::new(HandlePacketErrorKind::Ignorable, err))?;
            {
                let mut name = self.name.write();
                *name = reader
                    .read_str()
                    .map_err(|err| HandlePacketError::new(HandlePacketErrorKind::Ignorable, err))?;
            }
            reader
                .read_is_str()
                .map_err(|err| HandlePacketError::new(HandlePacketErrorKind::Ignorable, err))?;
            reader
                .read_str()
                .map_err(|err| HandlePacketError::new(HandlePacketErrorKind::Ignorable, err))?;
            {
                let mut register_player_id = self.register_player_id.write();
                *register_player_id = Some(reader.read_str().map_err(|err| {
                    HandlePacketError::new(HandlePacketErrorKind::Ignorable, err)
                })?);
            }
            Ok(())
        } else if let RelayPermissionStatus::PlayerPermission
        | RelayPermissionStatus::PlayerJoinPermission
        | RelayPermissionStatus::HostPermission
        | RelayPermissionStatus::Debug = permission_status
        {
            if let RelayPermissionStatus::PlayerPermission = permission_status {
                if let Some(relay_room) = self.relay_room.as_ref() {
                    if !relay_room
                        .is_start_game
                        .load(std::sync::atomic::Ordering::SeqCst)
                    {
                        let register_player_id = self.register_player_id.read().clone();
                        let kicked = relay_room
                            .conn_map
                            .iter()
                            .find_map(|pair| {
                                let conn = pair.value();
                                if let Some(conn) = conn.upgrade() {
                                    if let RelayPermissionStatus::PlayerJoinPermission =
                                        conn.permission_status()
                                    {
                                        let conn_register_player_id =
                                            conn.register_player_id.read();
                                        if register_player_id == *conn_register_player_id {
                                            self.kick(
                                                "[UUID Check] HEX repeated, just try another room",
                                            );
                                            return Some(Ok(()));
                                        }
                                    }
                                    None
                                } else {
                                    Some(Err(HandlePacketError::new(
                                        HandlePacketErrorKind::Ignorable,
                                        "expired conn not cleaned in relay_register_connection",
                                    )))
                                }
                            })
                            .transpose()?
                            .is_some();
                        if kicked {
                            return Ok(());
                        }
                    }

                    if self.player_relay.read().is_none() {
                        let mut player_relay = self.player_relay.write();
                        let register_player_id = self
                            .register_player_id
                            .read()
                            .clone()
                            .expect("no register player id!");
                        *player_relay = Some(
                            relay_room
                                .relay_players_data
                                .get(&register_player_id)
                                .map(|pair| pair.value().clone())
                                .unwrap_or_else(|| {
                                    let player_relay = Arc::new(PlayerRelay::new(
                                        Arc::downgrade(self),
                                        register_player_id.clone(),
                                        self.name.read().clone(),
                                    ));
                                    player_relay.set_now_name(self.name.read().clone());
                                    player_relay.set_disconnect(false);
                                    relay_room
                                        .relay_players_data
                                        .insert(register_player_id.clone(), player_relay.clone());
                                    player_relay
                                }),
                        );
                    }

                    // TODO: BAN,KICK,SYNC

                    let mut permission_status = self.permission_status.write();
                    *permission_status = RelayPermissionStatus::PlayerJoinPermission;
                    self.send_packet_to_host(packet);
                } else {
                    Err(HandlePacketError::new(
                        HandlePacketErrorKind::Fatal,
                        "no relay room in relay_register_connection",
                    ))?
                }
            }
            Ok(())
        } else {
            Ok(())
        }
    }

    fn kick(&self, msg: &str) {
        todo!()
    }

    fn send_packet_to_host(&self, packet: Packet) {
        todo!()
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
    relay_players_data: DashMap<String, Arc<PlayerRelay>>,
}

pub struct PlayerRelay {
    conn: RwLock<Weak<RelayConnection>>,
    uuid: String,
    name: String,
    now_name: RwLock<String>,
    disconnect: RwLock<bool>,
}

impl PlayerRelay {
    fn new(conn: Weak<RelayConnection>, uuid: String, name: String) -> Self {
        Self {
            conn: RwLock::new(conn),
            uuid,
            name,
            now_name: RwLock::new("".to_string()),
            disconnect: RwLock::new(false),
        }
    }

    fn set_now_name(&self, now_name: String) {
        let mut self_now_name = self.now_name.write();
        *self_now_name = now_name;
    }

    fn set_conn(&self, conn: Weak<RelayConnection>) {
        let mut self_conn = self.conn.write();
        *self_conn = conn;
    }

    fn set_disconnect(&self, disconnect: bool) {
        let mut self_disconnect = self.disconnect.write();
        *self_disconnect = disconnect;
    }
}
