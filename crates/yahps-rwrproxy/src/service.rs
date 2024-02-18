use std::{net::SocketAddr, sync::OnceLock};

use async_trait::async_trait;
use log::{debug, info, warn};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};

use crate::packet::Packet;

use yahps::service::{self, AddrTarget, GlobalConnectionHandle, Service};

pub struct ReverseProxy {
    target: SocketAddr,
    global_conn: GlobalConnectionHandle<Packet, SocketAddr>,
}

impl ReverseProxy {
    pub fn new<A>(target: A) -> Self
    where
        A: Into<SocketAddr>,
    {
        Self {
            target: target.into(),
            global_conn: GlobalConnectionHandle::default(),
        }
    }
}

impl Service for ReverseProxy {
    type Packet = Packet;
    type Handler = Handler;
    type Decoder = Decoder;
    type Encoder = Encoder;
    type LocalData = OnceLock<ReverseProxyConnection>;
    type Addr = SocketAddr;

    fn init(&mut self, global_conn: service::GlobalConnectionHandle<Self::Packet, Self::Addr>) {
        self.global_conn = global_conn;
    }

    fn create_decoder(&self) -> Self::Decoder {
        Decoder
    }

    fn create_encoder(&self) -> Self::Encoder {
        Encoder
    }

    fn create_handler(&self) -> Self::Handler {
        Handler::new(self.target, self.global_conn.clone())
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
        decode(reader).await
    }
}

async fn decode(reader: &mut (dyn AsyncRead + Unpin + Send)) -> Result<Packet, service::Error> {
    Packet::read_from_net(&mut Box::new(reader))
        .await
        .map_err(|err| {
            service::Error::new(
                match err.kind() {
                    std::io::ErrorKind::UnexpectedEof => service::ErrorKind::Disconnect,
                    _ => service::ErrorKind::WarnOnly,
                },
                err,
            )
        })
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
        encode(writer, packet).await
    }
}

async fn encode(
    writer: &mut (dyn AsyncWrite + Unpin + Send),
    packet: &Packet,
) -> Result<(), service::Error> {
    packet
        .write_into_net(&mut Box::new(writer))
        .await
        .map_err(|err| service::Error::new(service::ErrorKind::WarnOnly, err))
}

pub struct Handler {
    target: SocketAddr,
    global_conn: GlobalConnectionHandle<Packet, SocketAddr>,
}

impl Handler {
    pub fn new<A>(target: A, global_conn: GlobalConnectionHandle<Packet, SocketAddr>) -> Self
    where
        A: Into<SocketAddr>,
    {
        Self {
            target: target.into(),
            global_conn,
        }
    }
}

impl service::Handler for Handler {
    type Packet = Packet;
    type Local = OnceLock<ReverseProxyConnection>;
    type Addr = SocketAddr;

    fn handle(
        &mut self,
        packet: Self::Packet,
        conn: service::ConnectionHandle<Self::Packet, Self::Addr>,
        local: std::sync::Arc<Self::Local>,
    ) -> Result<(), service::Error> {
        debug!("handle: {:?}", packet.ty());
        if let Some(rconn) = local.get() {
            rconn.send_packet(packet);
        } else {
            match ReverseProxyConnection::connect(
                *conn.addr(),
                self.target,
                self.global_conn.clone(),
            ) {
                Err(err) => {
                    warn!("reverse proxy connection failed: {:?}", err);
                    conn.disconnect();
                }
                Ok(rconn) => {
                    debug!("prepare packet to send: {:?}", packet.ty());
                    rconn.send_packet(packet);
                    let _ = local.set(rconn);
                }
            }
        }
        Ok(())
    }
}

pub struct ReverseProxyConnection {
    packet_tx: std::sync::mpsc::Sender<Packet>,
}

impl ReverseProxyConnection {
    pub fn connect<A>(
        client_addr: A,
        server_addr: A,
        global_conn: GlobalConnectionHandle<Packet, SocketAddr>,
    ) -> Result<Self, std::io::Error>
    where
        A: Into<SocketAddr>,
    {
        let rt = tokio::runtime::Handle::try_current()
            .or_else(|_| tokio::runtime::Runtime::new().map(|run| run.handle().clone()))?;
        let client_addr: SocketAddr = client_addr.into();
        let server_addr: SocketAddr = server_addr.into();
        let disconnect_fn = {
            let global_conn = global_conn.clone();
            move || {
                global_conn.disconnect(AddrTarget::Only(client_addr));
            }
        };
        let send_packet_fn = {
            let global_conn = global_conn.clone();
            move |packet| {
                global_conn.send_packet(AddrTarget::Only(client_addr), packet);
            }
        };
        let (send_packet_tx, send_packet_rx) = std::sync::mpsc::channel::<Packet>();
        let _join_handle = {
            let disconnect_fn = disconnect_fn.clone();
            let send_packet_fn = send_packet_fn.clone();
            rt.spawn(async move {
                match TcpStream::connect(server_addr).await {
                    Err(err) => {
                        warn!("connect to server failed: {:?}", err);
                        disconnect_fn();
                    }
                    Ok(stream) => {
                        let (mut rx, mut tx) = stream.into_split();
                        let join_handle_client = {
                            let disconnect_fn = disconnect_fn.clone();
                            tokio::spawn(async move {
                                loop {
                                    match send_packet_rx.recv() {
                                        Err(_) => {
                                            info!("disconnect from client");
                                            disconnect_fn();
                                            break;
                                        }
                                        Ok(packet) => {
                                            if let Err(err) = packet.write_into_net(&mut tx).await {
                                                warn!("send packet failed: {:?}", err);
                                            }
                                        }
                                    }
                                }
                            })
                        };
                        let join_handle_server = tokio::spawn(async move {
                            loop {
                                match Packet::read_from_net(&mut rx).await {
                                    Err(_) => {
                                        info!("disconnected from server");
                                        disconnect_fn();
                                        break;
                                    }
                                    Ok(packet) => {
                                        debug!("recv from server: {:?}", packet.ty());
                                        send_packet_fn(packet);
                                    }
                                }
                            }
                        });
                        let _ = tokio::join!(join_handle_client, join_handle_server);
                    }
                }
            })
        };
        Ok(Self {
            packet_tx: send_packet_tx,
        })
    }

    pub fn send_packet(&self, packet: Packet) {
        if let Err(err) = self.packet_tx.send(packet) {
            warn!("{:?}", err);
        }
    }
}
