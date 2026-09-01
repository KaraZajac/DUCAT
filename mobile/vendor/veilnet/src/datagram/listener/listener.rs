//! Server for receiving messages at DHT addresses.

use tokio::{select, sync::watch};
use tokio_util::sync::CancellationToken;
use tracing::{error, instrument, warn};
use veilid_core::{
    BareRouteId, CRYPTO_KIND_VLD0, DHTSchema, KeyPair, PublicKey, RouteId, VeilidAppMessage,
    VeilidRouteChange,
};

use crate::{
    DHTAddr,
    connection::{self, API, Connected, Connection, RoutingContext, UpdateHandler},
    proto::{DHTRouteData, Encoder},
};

use super::{Error, Result};

/// A server that listens for incoming messages at a DHT address.
///
/// The listener binds to a DHT address by creating a DHT record and publishing
/// a private route to it. It then receives messages sent to that route through
/// the Veilid network. The listener automatically handles route management
/// and can recover from route failures.
///
/// # Examples
///
/// ```no_run
/// use veilnet::{connection::Veilid, datagram::Listener};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let conn = Veilid::new().await?;
/// let mut listener = Listener::new(conn, None, 8080).await?;
///
/// println!("Listening at: {}", listener.addr());
///
/// // Receive messages
/// let (sender_route, message) = listener.recv_from().await?;
/// println!("Got message: {:?}", message);
/// # Ok(())
/// # }
/// ```
pub struct Listener<C: Connection + Send> {
    conn: C,
    addr: DHTAddr,
    owner_keypair: KeyPair,

    cancel: CancellationToken,
    app_message_rx: flume::Receiver<VeilidAppMessage>,
    route_id_tx: watch::Sender<RouteId>,
    route_id_rx: watch::Receiver<RouteId>,
}

impl<C: Connection + Send + 'static> Listener<C> {
    /// Binds a listener to a DHT address.
    ///
    /// Creates a new DHT record and publishes a private route to it at the
    /// specified port (subkey). If an owner keypair is provided, the DHT
    /// record will be owned and can be updated. Otherwise, it creates an
    /// anonymous record.
    ///
    /// # Arguments
    ///
    /// * `conn` - The Veilid connection to use
    /// * `owner` - Optional keypair to own the DHT record
    /// * `port` - Port number (used as DHT subkey)
    ///
    /// # Returns
    ///
    /// A listener bound to the generated DHT address.
    pub async fn new(mut conn: C, owner_keypair: Option<KeyPair>, port: u16) -> Result<Self> {
        conn.require_attachment().await?;

        let owner_keypair = match owner_keypair {
            Some(keypair) => keypair,
            None => conn
                .with_crypto(|crypto| Ok(crypto.generate_keypair()))
                .map_err(|e| Error::Connection(connection::Error::Veilid(e)))?,
        };
        let rc = conn.routing_context();
        let rec = rc
            .create_dht_record(
                conn.crypto_kind(),
                DHTSchema::dflt(32)?,
                Some(owner_keypair.clone()),
            )
            .await?;
        let addr = DHTAddr {
            key: rec.key().to_owned(),
            subkey: port,
        };
        drop(rc);

        let cancel = CancellationToken::new();
        let route_id = Self::bind_route(&mut conn, owner_keypair.key(), &addr).await?;

        let (listener_watcher, app_message_rx, route_id_tx, route_id_rx) =
            ListenerWatcher::new(cancel.clone(), route_id);
        conn.add_update_handler(Box::new(listener_watcher));

        Ok(Listener {
            conn,
            addr,
            owner_keypair,
            cancel,
            app_message_rx,
            route_id_tx,
            route_id_rx,
        })
    }

    /// Creates a new private route and updates the DHT record.
    ///
    /// This can be used to recover from route failures or refresh the
    /// listener's connectivity. Returns the new route ID.
    async fn bind_route(conn: &mut C, owner_key: PublicKey, addr: &DHTAddr) -> Result<RouteId> {
        conn.require_attachment().await?;
        let rc = conn.routing_context();
        let route_blob = rc.api().new_private_route().await?;
        let dht_route = DHTRouteData::new(route_blob.blob, owner_key.clone());
        rc.set_dht_value(
            addr.key.to_owned(),
            addr.subkey.into(),
            dht_route.encode()?,
            None,
        )
        .await?;
        Ok(route_blob.route_id)
    }

    pub fn addr(&self) -> &DHTAddr {
        &self.addr
    }

    pub fn owner_key(&self) -> PublicKey {
        self.owner_keypair.key()
    }

    pub fn owner_keypair(&self) -> KeyPair {
        self.owner_keypair.clone()
    }

    #[instrument(skip_all)]
    pub async fn recv_from(&mut self) -> Result<(RouteId, Vec<u8>)> {
        if self
            .route_id_rx
            .borrow_and_update()
            .ref_value()
            .first_nonzero_bit()
            .is_none()
        {
            return Err(Error::DeadRoute);
        }

        self.conn.require_attachment().await?;
        loop {
            select! {
                _ = self.cancel.cancelled() => {
                    return Err(Error::Closed);
                }
                res = self.app_message_rx.recv_async() => {
                    let message = res.map_err(|e| Error::Watcher(Box::new(e)))?;
                    if let Some(route_id) = message.route_id() {
                        return Ok((route_id.to_owned(), message.message().to_vec()));
                    }
                }
                res = self.route_id_rx.changed() => {
                    res.map_err(crate::connection::Error::WatchReceive)?;
                    if self.route_id_rx.borrow_and_update().ref_value().first_nonzero_bit().is_none() {
                        return Err(Error::DeadRoute);
                    }
                }
            }
        }
    }

    pub async fn rebind(&mut self) -> Result<RouteId> {
        let route_id =
            Self::bind_route(&mut self.conn, self.owner_keypair.key(), &self.addr).await?;
        Ok(self.route_id_tx.send_replace(route_id))
    }

    #[instrument(skip_all)]
    pub async fn close(mut self) -> Result<()> {
        let rc = self.conn.routing_context();
        self.cancel.cancel();

        let route_id = self.route_id_rx.borrow_and_update().to_owned();
        if route_id.ref_value().first_nonzero_bit().is_some()
            && let Err(err) = rc.api().release_private_route(route_id)
        {
            warn!(?err);
        }
        if let Err(err) = rc.close_dht_record(self.addr.key.to_owned()).await {
            warn!(?err, %self.addr, "close_dht_record")
        }
        if let Err(err) = rc.delete_dht_record(self.addr.key.to_owned()).await {
            warn!(?err, %self.addr, "delete_dht_record")
        }
        drop(rc);
        self.conn.close().await?;
        Ok(())
    }
}

impl<C: Connection + Send> Connected<C> for Listener<C> {
    /// Gets a reference to the underlying connection.
    fn conn(&self) -> &C {
        &self.conn
    }

    /// Gets a mutable reference to the underlying connection.
    fn conn_mut(&mut self) -> &mut C {
        &mut self.conn
    }
}

struct ListenerWatcher {
    cancel: CancellationToken,
    app_message_tx: flume::Sender<VeilidAppMessage>,
    route_id_tx: watch::Sender<RouteId>,
}

impl ListenerWatcher {
    pub fn new(
        cancel: CancellationToken,
        route_id: RouteId,
    ) -> (
        ListenerWatcher,
        flume::Receiver<VeilidAppMessage>,
        watch::Sender<RouteId>,
        watch::Receiver<RouteId>,
    ) {
        let (app_message_tx, app_message_rx) = flume::unbounded();
        let (route_id_tx, route_id_rx) = watch::channel(route_id);
        (
            Self {
                cancel,
                app_message_tx,
                route_id_tx: route_id_tx.clone(),
            },
            app_message_rx,
            route_id_tx,
            route_id_rx,
        )
    }
}

impl UpdateHandler for ListenerWatcher {
    fn is_done(&self) -> bool {
        self.app_message_tx.is_disconnected()
    }
    fn app_message(&self, message: &VeilidAppMessage) {
        if let Err(err) = self.app_message_tx.send((*message).clone()) {
            error!(
                ?err,
                "failed to send to app_message channel (unrecoverable)"
            );
            self.cancel.cancel();
        }
    }

    fn route_change(&self, route_change: &VeilidRouteChange) {
        self.route_id_tx.send_if_modified(|route_id: &mut RouteId| {
            if route_change.dead_routes.contains(route_id) {
                warn!(?route_id, "dead route");
                *route_id = RouteId::new(CRYPTO_KIND_VLD0, BareRouteId::default());
                true
            } else {
                false
            }
        });
    }

    fn shutdown(&self) {
        self.cancel.cancel();
    }
}
