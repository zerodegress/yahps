use dashmap::DashMap;
use log::warn;
use std::{net::SocketAddr, sync::Arc, time::SystemTime};
use tokio::{net::TcpListener, sync::broadcast, task::JoinSet};

use crate::service::{AddrTarget, ConnectionHandle, Decoder, Encoder, ErrorKind, Handler, Service};

pub struct Server<S>
where
    S: Service<Addr = SocketAddr>,
{
    binds: Vec<SocketAddr>,
    worker_count: usize,
    service: S,
}

impl<S> Server<S>
where
    S: Service<Addr = SocketAddr>,
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

    pub async fn run(self) -> Result<(), std::io::Error> {
        let (disconnect_broadcast_tx, _) = broadcast::channel::<AddrTarget<S::Addr>>(64);
        let (send_packet_broadcast_tx, _) =
            broadcast::channel::<(AddrTarget<S::Addr>, S::Packet)>(64);
        let (packet_tx, packet_rx) =
            async_channel::bounded::<(S::Packet, SocketAddr, SystemTime)>(self.worker_count);
        let local_data_map = Arc::new(DashMap::<SocketAddr, Arc<S::LocalData>>::new());
        let service = Arc::new(self.service);
        let mut join_set = JoinSet::new();

        for _ in 0..self.worker_count {
            let disconnect_broadcast_tx = disconnect_broadcast_tx.clone();
            let send_packet_broadcast_tx = send_packet_broadcast_tx.clone();
            let packet_rx = packet_rx.clone();
            let conn_data_map = local_data_map.clone();
            let service = service.clone();
            join_set.spawn(async move {
                let mut handler = service.create_handler();
                let send_packet_fn = move |target, packet| {
                    if let Err(err) = send_packet_broadcast_tx.send((target, packet)) {
                        warn!("{}", err);
                    }
                };
                loop {
                    match packet_rx.recv().await {
                        Err(err) => {
                            warn!("{}", err);
                        }
                        Ok((packet, addr, time)) => {
                            let disconnect_broadcast_tx = disconnect_broadcast_tx.clone();
                            let conn = ConnectionHandle::new(
                                addr,
                                time,
                                move || {
                                    if let Err(err) =
                                        disconnect_broadcast_tx.send(AddrTarget::Only(addr))
                                    {
                                        warn!("{}", err);
                                    }
                                },
                                send_packet_fn.clone(),
                            );
                            let local =
                                conn_data_map
                                    .get(&addr)
                                    .map(|x| x.clone())
                                    .unwrap_or_else(|| {
                                        let local = Arc::new(service.init_local());
                                        conn_data_map.insert(addr, local.clone());
                                        local
                                    });
                            if let Err(err) = handler.handle(packet, conn, local) {
                                warn!("{}", err);
                            }
                        }
                    }
                }
            });
        }

        for bind in self.binds {
            let disconnect_broadcast_tx = disconnect_broadcast_tx.clone();
            let listener = TcpListener::bind(bind).await?;
            let service = service.clone();
            let conn_broadcast_tx = send_packet_broadcast_tx.clone();
            let packet_tx = packet_tx.clone();
            join_set.spawn(async move {
                let mut join_set = JoinSet::new();
                loop {
                    let disconnect_broadcast_tx = disconnect_broadcast_tx.clone();
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
                            {
                                let (disconnect_broadcast_tx, mut disconnect_broadcast_rx) = {
                                    let rx = disconnect_broadcast_tx.clone().subscribe();
                                    let tx = disconnect_broadcast_tx.clone();
                                    (tx, rx)
                                };
                                join_set.spawn(async move {
                                    loop {
                                        tokio::select! {
                                            disc = disconnect_broadcast_rx.recv() => {
                                                match disc {
                                                    Err(err) => {
                                                        warn!("{}", err);
                                                    }
                                                    Ok(target) => match target {
                                                        AddrTarget::All => {
                                                            break;
                                                        }
                                                        AddrTarget::Only(only_addr) if addr == only_addr => {
                                                            break;
                                                        }
                                                        AddrTarget::Without(without_addr) if addr != without_addr => {
                                                            break;
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                            }
                                            res = decoder.decode(&mut stream_rx) => {
                                                match res {
                                                    Err(err) => {
                                                        warn!("{}", err);
                                                        match err.kind() {
                                                            ErrorKind::Disconnect => {
                                                                if let Err(err) = disconnect_broadcast_tx.send(AddrTarget::Only(addr)) {
                                                                    warn!("{}", err);
                                                                }
                                                            }
                                                            ErrorKind::Fatal => {
                                                                panic!();
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                    Ok(packet) => {
                                                        if let Err(err) =
                                                            packet_tx.send((packet, addr, SystemTime::now())).await
                                                        {
                                                            warn!("{}", err);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                });
                            }
                            {
                                let (disconnect_broadcast_tx, mut disconnect_broadcast_rx) = {
                                    let rx = disconnect_broadcast_tx.clone().subscribe();
                                    let tx = disconnect_broadcast_tx.clone();
                                    (tx, rx)
                                };
                                join_set.spawn(async move {
                                    loop {
                                        tokio::select! {
                                            disc = disconnect_broadcast_rx.recv() => {
                                                match disc {
                                                    Err(err) => {
                                                        warn!("{}", err);
                                                    }
                                                    Ok(target) => match target {
                                                        AddrTarget::All => {
                                                            break;
                                                        }
                                                        AddrTarget::Only(only_addr) if addr == only_addr => {
                                                            break;
                                                        }
                                                        AddrTarget::Without(without_addr) if addr != without_addr => {
                                                            break;
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                            }
                                            res = conn_broadcast_rx.recv() => {
                                                match res {
                                                    Err(err) => {
                                                        warn!("{}", err)
                                                    }
                                                    Ok((target, packet)) => {
                                                        let send = async {
                                                            if let Err(err) =
                                                                encoder.encode(&mut stream_tx, &packet).await
                                                            {
                                                                warn!("{}", err);
                                                                match err.kind() {
                                                                    ErrorKind::Disconnect => {
                                                                        if let Err(err) = disconnect_broadcast_tx.send(AddrTarget::Only(addr)) {
                                                                            warn!("{}", err);
                                                                        }
                                                                    }
                                                                    ErrorKind::Fatal => {
                                                                        panic!();
                                                                    }
                                                                    _ => {}
                                                                }
                                                            }
                                                        };
                                                        match target {
                                                            AddrTarget::All => {
                                                                send.await;
                                                            }
                                                            AddrTarget::Only(only_addr) if addr == only_addr => {
                                                                send.await;
                                                            }
                                                            AddrTarget::Without(without_addr)
                                                                if addr != without_addr =>
                                                            {
                                                                send.await;
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                }
                                            }
                                        };
                                    }
                                });
                            }
                        }
                    }
                }
                // TODO: How to break and join
                // while (join_set.join_next().await).is_some() {}
            });
        }

        while (join_set.join_next().await).is_some() {}
        Ok(())
    }
}

pub trait ServerSocketAddr {}

impl ServerSocketAddr for SocketAddr {}
