//! Client for sending messages to DHT addresses.

use veilid_core::{PublicKey, RouteId, Target};

use crate::{
    DHTAddr,
    connection::{API, Connected, Connection, RoutingContext},
    proto::{DHTRouteData, Decoder},
};

use super::{Error, Result};

/// A client for sending messages to DHT addresses over Veilid private routes.
///
/// The dialer resolves DHT addresses to private routes and sends messages to them.
/// It manages the underlying Veilid connection and handles route resolution automatically.
///
/// # Examples
///
/// ```no_run
/// use veilnet::{connection::Veilid, DHTAddr, datagram::Dialer};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let conn = Veilid::new().await?;
/// let mut dialer = Dialer::new(conn).await?;
///
/// let addr: DHTAddr = "VLD0:abcd...efgh:8080".parse()?;
/// let (route_id, _) = dialer.resolve(&addr).await?;
/// dialer.send_to(route_id, b"Hello!".to_vec()).await?;
/// # Ok(())
/// # }
/// ```
pub struct Dialer<C: Connection + Send> {
    conn: C,
}

impl<C: Connection + Send> Dialer<C> {
    /// Creates a new dialer with the given connection.
    ///
    /// The connection must be attached and ready before the dialer can be used.
    /// This method will wait for attachment if necessary.
    pub async fn new(mut conn: C) -> Result<Self> {
        conn.require_attachment().await?;
        Ok(Dialer { conn })
    }

    pub async fn resolve_route_data(
        &mut self,
        DHTAddr { key, subkey }: &DHTAddr,
    ) -> Result<DHTRouteData> {
        self.conn.require_attachment().await?;
        let rc = self.conn.routing_context();
        let rec = rc.open_dht_record(key.to_owned(), None).await?;
        let route_data = rc
            .get_dht_value(rec.key().to_owned(), (*subkey).into(), true)
            .await?;
        match route_data {
            Some(route_data) => Ok(DHTRouteData::decode(route_data.data())?),
            None => Err(Error::RouteNotFound(DHTAddr {
                key: key.to_owned(),
                subkey: *subkey,
            })),
        }
    }

    pub async fn resolve(&mut self, addr: &DHTAddr) -> Result<(RouteId, PublicKey)> {
        let DHTRouteData {
            route_data,
            owner_key,
        } = self.resolve_route_data(addr).await?;
        let route_id = self
            .conn
            .routing_context()
            .api()
            .import_remote_private_route(route_data)?;
        Ok((route_id, owner_key))
    }

    pub async fn send_to(&mut self, route_id: RouteId, data: Vec<u8>) -> Result<()> {
        self.conn.require_attachment().await?;
        self.conn
            .routing_context()
            .app_message(Target::RouteId(route_id), data)
            .await?;
        Ok(())
    }

    pub async fn close(self) -> Result<()> {
        self.conn.close().await?;
        Ok(())
    }
}

impl<C: Connection + Send> Connected<C> for Dialer<C> {
    /// Gets a reference to the underlying connection.
    fn conn(&self) -> &C {
        &self.conn
    }

    /// Gets a mutable reference to the underlying connection.
    fn conn_mut(&mut self) -> &mut C {
        &mut self.conn
    }
}
