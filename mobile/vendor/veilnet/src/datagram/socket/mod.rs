use std::collections::HashMap;

use anyhow::{Result, anyhow, bail};

use backoff::{ExponentialBackoff, backoff::Backoff};
use tokio::time::sleep;
use veilid_core::{KeyPair, PublicKey, RouteId};

use crate::connection::Connected;
use crate::datagram::{dialer, listener};
use crate::proto::{Datagram, Decoder, Encoder, Packet};
use crate::{Connection, DHTAddr};

/// A datagram socket based on private routes published to the Veilid DHT.
pub struct Socket<C: Connection + Send + Sync + 'static> {
    dialer: dialer::Dialer<C>,
    listener: listener::Listener<C>,

    addr_routes: HashMap<DHTAddr, (RouteId, PublicKey)>,
    route_addrs: HashMap<RouteId, (PublicKey, DHTAddr)>,
}

impl<C: Connection + Clone + Send + Sync + 'static> Socket<C> {
    /// Create a datagram socket
    ///
    /// If the owner key pair is provided, the socket will have a stable,
    /// derived DHT address. Otherwise a new random address will be allocated.
    ///
    /// The port number corresponds to a DHT subkey used to store the private
    /// route data.
    pub async fn new(conn: C, owner: Option<KeyPair>, port: u16) -> Result<Self> {
        Ok(Self {
            dialer: dialer::Dialer::new(conn.clone()).await?,
            listener: listener::Listener::new(conn, owner, port).await?,
            addr_routes: HashMap::new(),
            route_addrs: HashMap::new(),
        })
    }

    /// Get the public DHT address for the socket.
    pub fn addr(&self) -> &DHTAddr {
        self.listener.addr()
    }

    /// Get the owner public key that can write to the DHT for this socket's
    /// address.
    ///
    /// The owner key signs datagram packets.
    pub fn owner_key(&self) -> PublicKey {
        self.listener.owner_key()
    }

    /// Send a datagram to a public DHT address.
    ///
    /// The maximum upper bound for a Veilid app_message is 32Kib.
    ///
    /// The maximum length of the contents of the datagram is less than this; it
    /// must allow for the packet containing it, which includes the source
    /// address, owner key, and signature, as well as capnproto overhead.
    pub async fn send_to(&mut self, addr: &DHTAddr, data: &[u8]) -> Result<()> {
        let mut retry = ExponentialBackoff::default();
        let mut state = dialer::State::Unknown;
        loop {
            let inner = async {
                let (route_id, _) = match self.addr_routes.get(addr) {
                    Some((route_id, owner_key)) => (route_id.to_owned(), owner_key.to_owned()),
                    None => {
                        let (route_id, owner_key) = self.dialer.resolve(addr).await?;
                        self.addr_routes
                            .insert(addr.to_owned(), (route_id.to_owned(), owner_key.to_owned()));
                        (route_id, owner_key)
                    }
                };
                self.dialer
                    .send_to(
                        route_id,
                        self.listener
                            .conn()
                            .with_crypto(|crypto| {
                                let datagram =
                                    Datagram::new(self.addr().to_owned(), self.owner_key(), data);
                                Packet::new_signature(
                                    datagram,
                                    &crypto,
                                    self.listener.owner_keypair(),
                                )
                            })?
                            .encode()?,
                    )
                    .await?;
                Ok::<_, anyhow::Error>(())
            };
            match inner.await {
                Ok(_) => return Ok(()),
                Err(err) => {
                    match err.downcast_ref::<dialer::Error>() {
                        Some(dialer_err) => {
                            state = state.next_err(dialer_err);
                            match state {
                                dialer::State::NeedResolve { .. } => {
                                    self.addr_routes.remove(addr);
                                }
                                dialer::State::NeedReset => {
                                    self.dialer.conn_mut().reset().await?;
                                }
                                dialer::State::Unrecoverable => {
                                    return Err(err);
                                }
                                _ => continue,
                            }
                        }
                        None => {
                            return Err(err);
                        }
                    }

                    sleep(match retry.next_backoff() {
                        Some(delay) => delay,
                        None => return Err(err),
                    })
                    .await;
                }
            };
        }
    }

    /// Receive a datagram packet sent to this socket's public DHT address.
    pub async fn recv_from(&mut self) -> Result<(DHTAddr, Vec<u8>)> {
        let mut retry = ExponentialBackoff::default();
        let mut listener_state = listener::State::Unknown;
        let mut dialer_state = dialer::State::Unknown;
        loop {
            let inner = async {
                let (route_id, data) = self.listener.recv_from().await?;
                let packet = Packet::decode(data.as_slice())?;
                let datagram = packet.datagram()?;
                self.listener
                    .conn()
                    .with_crypto(|crypto| packet.verify(&datagram, &crypto))?;
                match self.route_addrs.get(&route_id) {
                    Some((owner_key, addr)) => {
                        if owner_key != &datagram.owner_key {
                            return Err(anyhow!("source key mismatch"));
                        }
                        Ok((addr.to_owned(), datagram.contents))
                    }
                    None => {
                        let dht_route = self.dialer.resolve_route_data(&datagram.addr).await?;
                        if dht_route.owner_key != datagram.owner_key {
                            return Err(anyhow!(
                                "packet source address {} claims record owner {} but was signed by {}",
                                datagram.addr,
                                dht_route.owner_key,
                                datagram.owner_key,
                            ));
                        }
                        self.route_addrs.insert(
                            route_id,
                            (datagram.owner_key.to_owned(), datagram.addr.to_owned()),
                        );
                        Ok::<_, anyhow::Error>((datagram.addr, datagram.contents))
                    }
                }
            };
            match inner.await {
                Ok(ok) => return Ok(ok),
                Err(err) => {
                    match (
                        err.downcast_ref::<dialer::Error>(),
                        err.downcast_ref::<listener::Error>(),
                    ) {
                        (Some(dialer_err), _) => {
                            dialer_state = dialer_state.next_err(dialer_err);
                            match dialer_state {
                                dialer::State::Healthy | dialer::State::Unknown => bail!(
                                    "unexpected dialer state {dialer_state} on dialer error {dialer_err}"
                                ),
                                dialer::State::Unrecoverable => return Err(err),
                                dialer::State::NeedReset => {
                                    self.dialer.conn_mut().reset().await?;
                                    retry.reset();
                                    continue;
                                }

                                // let other errors retry
                                _ => {}
                            }
                        }
                        (_, Some(listener_err)) => {
                            listener_state = listener_state.next_err(listener_err);
                            match listener_state {
                                listener::State::Healthy | listener::State::Unknown => bail!(
                                    "unexpected listener state {listener_state} on listener error {listener_err}"
                                ),
                                listener::State::NeedRebind { .. } => {
                                    match self.listener.rebind().await {
                                        Ok(_) => continue,
                                        Err(ref err) => {
                                            listener_state = listener_state.next_err(err);
                                        }
                                    };
                                }
                                listener::State::Unrecoverable => return Err(err),

                                // let others fall through
                                _ => {}
                            };
                            match listener_state {
                                listener::State::NeedReset => {
                                    self.listener.conn_mut().reset().await?;
                                    retry.reset();
                                    continue;
                                }
                                listener::State::Unrecoverable => return Err(err),

                                // let other errors retry
                                _ => {}
                            }
                        }
                        (None, None) => return Err(err),
                    }

                    sleep(match retry.next_backoff() {
                        Some(delay) => delay,
                        None => return Err(err),
                    })
                    .await;
                }
            };
        }
    }
}
