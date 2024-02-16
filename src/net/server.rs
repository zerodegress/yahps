use dashmap::DashMap;
use log::warn;
use std::{net::SocketAddr, sync::Arc, time::SystemTime};
use tokio::{net::TcpListener, sync::broadcast, task::JoinSet};

use crate::net::service::{ConnectionHandle, Decoder, Encoder};

use super::service::Service;

pub struct Server<S>
where
    S: Service,
{
    binds: Vec<SocketAddr>,
    worker_count: usize,
    service: S,
}

impl<S> Server<S>
where
    S: Service,
{
    pub fn new(service: S) -> Self {
        Self {
            binds: vec![],
            worker_count: num_cpus::get(),
            service,
        }
    }

    pub fn bind<Addr>(mut self, addr: Addr) -> Self
    where
        Addr: Into<SocketAddr>,
    {
        self.binds.push(addr.into());
        self
    }

    pub async fn run(self) -> Result<JoinSet<()>, std::io::Error> {
        let service = Arc::new(self.service);
        let (conn_broadcast_tx, conn_broadcast_rx) =
            broadcast::channel::<(ConnectionSendMessageType, S::Packet)>(64);
        let (packet_tx, packet_rx) =
            async_channel::bounded::<(S::Packet, SocketAddr, SystemTime)>(self.worker_count);
        let conn_data_map = Arc::new(DashMap::<SocketAddr, Arc<()>>::new());
        let global_data = None as Option<Arc<()>>;
        let mut join_set = JoinSet::new();
        for _ in 0..self.worker_count {
            let conn_broadcast_tx = conn_broadcast_tx.clone();
            let packet_rx = packet_rx.clone();
            let conn_data_map = conn_data_map.clone();
            let service = service.clone();
            join_set.spawn(async move {
                loop {
                    match packet_rx.recv().await {
                        Err(err) => {
                            warn!("{}", err);
                        }
                        Ok((packet, addr, time)) => {
                            let conn_handle = ConnectionHandle::new(addr, time, || todo!());
                            let local = conn_data_map.get(&addr).map(|x| x.clone());
                            todo!()
                        }
                    }
                }
            });
        }
        for bind in self.binds {
            let listener = TcpListener::bind(bind).await?;
            let service = service.clone();
            let conn_broadcast_tx = conn_broadcast_tx.clone();
            let packet_tx = packet_tx.clone();
            join_set.spawn(async move {
                loop {
                    let packet_tx = packet_tx.clone();
                    let decoder = service.create_decoder();
                    let encoder = service.create_encoder();
                    match listener.accept().await {
                        Err(err) => {
                            warn!("{}", err);
                        }
                        Ok((stream, addr)) => {
                            let mut conn_broadcast_rx = conn_broadcast_tx.subscribe();
                            let (mut stream_rx, mut stream_tx) = stream.into_split();
                            tokio::spawn(async move {
                                match decoder.decode(&mut stream_rx).await {
                                    Err(err) => {
                                        warn!("{}", err)
                                    }
                                    Ok(packet) => {
                                        if let Err(err) =
                                            packet_tx.send((packet, addr, SystemTime::now())).await
                                        {
                                            warn!("{}", err);
                                        }
                                    }
                                }
                            });
                            tokio::spawn(async move {
                                match conn_broadcast_rx.recv().await {
                                    Err(err) => {
                                        warn!("{}", err)
                                    }
                                    Ok((ty, packet)) => {
                                        let send = async move {
                                            if let Err(err) =
                                                encoder.encode(&mut stream_tx, &packet).await
                                            {
                                                warn!("{}", err);
                                            }
                                        };
                                        match ty {
                                            ConnectionSendMessageType::All => {
                                                send.await;
                                            }
                                            ConnectionSendMessageType::Only(only_addr)
                                                if addr == only_addr =>
                                            {
                                                send.await;
                                            }
                                            ConnectionSendMessageType::Without(without_addr)
                                                if addr != without_addr =>
                                            {
                                                send.await;
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            });
                        }
                    }
                }
            });
        }
        todo!()
    }
}

#[derive(Debug, Clone)]
pub enum ConnectionSendMessageType {
    All,
    Only(SocketAddr),
    Without(SocketAddr),
}
