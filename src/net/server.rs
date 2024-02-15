pub mod error;

use std::{
    net::SocketAddr,
    sync::{atomic::AtomicUsize, Arc},
};

use log::warn;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
    task::JoinSet,
};

use crate::{data::packet::Packet, net::service::error::HandlePacketErrorKind};

use self::error::WorkerError;

use super::{
    connection::Connection,
    service::{NeverService, Service},
};

pub trait PacketHandler {
    fn handle_packet(&self, conn: &mut Connection, packet: Packet) -> Result<(), WorkerError>;
}

impl<T> PacketHandler for T
where
    T: Fn(&mut Connection, Packet) -> Result<(), WorkerError>,
{
    fn handle_packet(&self, conn: &mut Connection, packet: Packet) -> Result<(), WorkerError> {
        self(conn, packet)
    }
}

pub struct Server {
    binds: Vec<SocketAddr>,
    default_worker_count: usize,
    service: Box<dyn Service + Send + Sync>,
}

impl Default for Server {
    fn default() -> Self {
        Self {
            binds: vec![],
            default_worker_count: num_cpus::get(),
            service: Box::new(NeverService),
        }
    }
}

impl Server {
    pub fn bind<Addr>(mut self, addr: Addr) -> Self
    where
        Addr: Into<SocketAddr>,
    {
        self.binds.push(addr.into());
        self
    }

    pub fn service<S>(mut self, service: S) -> Self
    where
        S: Service + Send + Sync + 'static,
    {
        self.service = Box::new(service);
        self
    }

    pub async fn run(self) {
        let free_worker_count = Arc::new(AtomicUsize::new(0));
        let service = Arc::new(self.service);
        let (conn_tx, conn_rx) = async_channel::unbounded::<(TcpStream, SocketAddr)>();
        let mut worker_joinset = JoinSet::new();

        (0..self.default_worker_count).for_each(|_| {
            let rx = conn_rx.clone();
            let free_worker_count = free_worker_count.clone();
            let service = service.clone();

            worker_joinset.spawn(async move {
                loop {
                    let service = service.clone();
                    free_worker_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    match rx.recv().await {
                        Err(err) => {
                            // TODO: more error processing
                            warn!("{}", err);
                            break;
                        }
                        Ok((stream, addr)) => {
                            free_worker_count.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                            let (sr, mut sw) = stream.into_split();
                            let (send_packet_tx, mut send_packet_rx) = mpsc::channel(64);
                            let conn = Connection::connect(addr, sr, send_packet_tx.clone());
                            let recv_task = tokio::spawn(async move {
                                let mut service_handler = service.create_handler(conn);
                                loop {
                                    match service_handler.read_packet().await {
                                        Err(err) => {
                                            // TODO: more error processing
                                            warn!("{}", err);
                                            break;
                                        }
                                        Ok(packet) => {
                                            if let Err(err) =
                                                service_handler.handle_packet(&packet).await
                                            {
                                                match err.kind() {
                                                    HandlePacketErrorKind::Disconnect => break,
                                                    HandlePacketErrorKind::Fatal => {
                                                        // TODO: more exactly
                                                        panic!("{}", err)
                                                    }
                                                    HandlePacketErrorKind::Ignorable => {}
                                                }
                                            }
                                        }
                                    }
                                }
                            });
                            let _ = tokio::join!(
                                tokio::spawn(async move {
                                    while let Some(packet) = send_packet_rx.recv().await {
                                        if let Err(err) = packet.write_into_net(&mut sw).await {
                                            // TODO: more error processing
                                            warn!("{}", err);
                                            break;
                                        }
                                    }
                                }),
                                recv_task
                            );
                        }
                    }
                }
            });
        });

        let mut boss_joinset = JoinSet::new();
        self.binds.into_iter().for_each(|bind| {
            let conn_tx = conn_tx.clone();
            boss_joinset.spawn(async move {
                match TcpListener::bind(bind).await {
                    Err(err) => {
                        // TODO: more error processing
                        warn!("{}", err);
                    }
                    Ok(listener) => loop {
                        match listener.accept().await {
                            Err(err) => {
                                // TODO: more error processing
                                warn!("{}", err);
                                break;
                            }
                            Ok((stream, addr)) => {
                                if let Err(err) = conn_tx.send((stream, addr)).await {
                                    // TODO: more error processing
                                    warn!("{}", err);
                                    break;
                                }
                            }
                        }
                    },
                }
            });
        });

        while boss_joinset.join_next().await.is_some() {}
        while worker_joinset.join_next().await.is_some() {}

        todo!()
    }
}
