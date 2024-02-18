use dashmap::DashMap;
use log::{debug, trace, warn};
use std::{net::SocketAddr, sync::Arc, time::SystemTime};
use tokio::{net::TcpListener, sync::broadcast, task::JoinSet};

use crate::service::{AddrTarget, Decoder, Encoder, ErrorKind, Handler, Service};

use super::connection::{ConnectionHandle, GlobalConnectionHandle};

pub struct Server<P, S>
where
    P: Clone + Send + Sync + 'static,
    S: Service<Packet = P, Addr = SocketAddr, GlobalConnectionHandle = GlobalConnectionHandle<P>>
        + 'static,
    S::Handler: Handler<ConnectionHandle = ConnectionHandle<P>>,
{
    binds: Vec<SocketAddr>,
    worker_count: usize,
    service: S,
}

impl<P, S> Server<P, S>
where
    P: Clone + Send + Sync + 'static,
    S: Service<Packet = P, Addr = SocketAddr, GlobalConnectionHandle = GlobalConnectionHandle<P>>
        + 'static,
    S::Handler: Handler<ConnectionHandle = ConnectionHandle<P>>,
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
            broadcast::channel::<(AddrTarget<S::Addr>, Arc<S::Packet>)>(64);
        let (packet_tx, packet_rx) =
            async_channel::bounded::<(S::Packet, SocketAddr, SystemTime)>(self.worker_count);
        let local_data_map =
            Arc::new(DashMap::<SocketAddr, Arc<<S::Handler as Handler>::Local>>::new());

        let mut service = self.service;
        {
            let (disconnect_broadcast_tx, send_packet_broadcast_tx) = (
                disconnect_broadcast_tx.clone(),
                send_packet_broadcast_tx.clone(),
            );
            service.init(GlobalConnectionHandle::new(
                move |target| {
                    if let Err(err) = disconnect_broadcast_tx.send(target) {
                        debug!("send disconnect failed from service: {:?}", err);
                    }
                },
                move |target, packet| {
                    if let Err(err) = send_packet_broadcast_tx.send((target, Arc::new(packet))) {
                        debug!("send packet failed from service: {}", err);
                    }
                },
            ));
        }
        let service = Arc::new(service);

        let mut join_set = JoinSet::new();

        for _ in 0..self.worker_count {
            let disconnect_broadcast_tx = disconnect_broadcast_tx.clone();
            let send_packet_broadcast_tx = send_packet_broadcast_tx.clone();
            let packet_rx = packet_rx.clone();
            let conn_data_map = local_data_map.clone();
            let service = service.clone();
            join_set.spawn(async move {
                let mut handler = service.create_handler();
                loop {
                    let send_packet_broadcast_tx = send_packet_broadcast_tx.clone();
                    match packet_rx.recv().await {
                        Err(err) => {
                            warn!("{}", err);
                        }
                        Ok((packet, addr, time)) => {
                            let send_packet_fn = move |packet| {
                                if let Err(err) = send_packet_broadcast_tx
                                    .send((AddrTarget::Only(addr), Arc::new(packet)))
                                {
                                    warn!("{}", err);
                                }
                            };
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
                                send_packet_fn,
                            );
                            let local =
                                conn_data_map
                                    .get(&addr)
                                    .map(|x| x.clone())
                                    .unwrap_or_else(|| {
                                        let local =
                                            Arc::new(<S::Handler as Handler>::Local::default());
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
            let local_data_map = local_data_map.clone();
            let disconnect_broadcast_tx = disconnect_broadcast_tx.clone();
            let listener = TcpListener::bind(bind).await?;
            let service = service.clone();
            let conn_broadcast_tx = send_packet_broadcast_tx.clone();
            let packet_tx = packet_tx.clone();
            join_set.spawn(async move {
                let mut join_set = JoinSet::new();
                loop {
                    let local_data_map = local_data_map.clone();
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
                                                        warn!("recv disconnect signal failed: {:?}", err);
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
                                                        match err.kind() {
                                                            ErrorKind::Ignorable => {
                                                                debug!("decode packet with ignorable error: {:?}", err);
                                                            }
                                                            ErrorKind::WarnOnly => {
                                                                warn!("decode packet with warn: {:?}", err);
                                                            }
                                                            ErrorKind::Disconnect => {
                                                                if let Err(err) = disconnect_broadcast_tx.send(AddrTarget::Only(addr)) {
                                                                    warn!("decode packet failed and disconnect: {:?}", err);
                                                                }
                                                            }
                                                            ErrorKind::Fatal => {
                                                                panic!("decode packet failed and panic: {:?}", err);
                                                            }
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
                                    local_data_map.remove(&addr);
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
                                                        warn!("recv disconnect signal failed: {:?}", err);
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
                                                        trace!("packet to send ready, target: {:?}, now addr: {:?}", target, addr);
                                                        let send = async {
                                                            if let Err(err) =
                                                                encoder.encode(&mut stream_tx, &packet).await
                                                            {
                                                                warn!("{}", err);
                                                                match err.kind() {
                                                                    ErrorKind::Ignorable => {
                                                                        debug!("encode packet with ignorable Error: {:?}", err);
                                                                    }
                                                                    ErrorKind::WarnOnly => {
                                                                        warn!("encode packet with warn: {:?}", err);
                                                                    }
                                                                    ErrorKind::Disconnect => {
                                                                        if let Err(err) = disconnect_broadcast_tx.send(AddrTarget::Only(addr)) {
                                                                            warn!("encode packet failed and disconnect: {:?}", err);
                                                                        }
                                                                    }
                                                                    ErrorKind::Fatal => {
                                                                        panic!("encode packet failed and panic: {:?}", err);
                                                                    }
                                                                }
                                                            }
                                                        };
                                                        match target {
                                                            AddrTarget::All => {
                                                                send.await;
                                                            }
                                                            AddrTarget::Only(only_addr) if addr == only_addr => {
                                                                trace!("packet sent to: {}", only_addr);
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
