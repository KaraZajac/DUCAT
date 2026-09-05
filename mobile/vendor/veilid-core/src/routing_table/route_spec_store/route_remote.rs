use super::*;

impl RouteSpecStore {
    /// Choose the best private route from a private route set to communicate with
    pub fn best_remote_private_route(&self, id: &RemoteRouteSetId) -> Option<Arc<PrivateRoute>> {
        let mut cache = self.cache.write();
        let cur_ts = Timestamp::now();
        let rrce = cache.get_remote_route(cur_ts, id)?;
        rrce.best_private_route()
    }

    /// Check if a route id is remote or not
    pub fn is_route_id_remote(&self, id: &RouteId) -> bool {
        let cache = self.cache.read();
        let cur_ts = Timestamp::now();
        cache
            .peek_remote_route(cur_ts, &RemoteRouteSetId::from_route_id(id.clone()))
            .is_some()
    }

    /// Import a remote private route set blob for compilation
    /// It is safe to import the same route more than once and it will return the same route id
    /// Returns a route set id
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub fn import_remote_route_blob(&self, blob: Vec<u8>) -> VeilidAPIResult<RouteId> {
        let cur_ts = Timestamp::now();

        // decode the pr blob
        let private_routes = self.blob_to_private_routes(blob)?;

        // make the route id
        let id = self.generate_remote_route_id(&private_routes)?;

        // validate the private routes
        let mut cache = self.cache.write();
        for private_route in &private_routes {
            // ensure private route has first hop
            if !matches!(private_route.hops, PrivateRouteHops::FirstHop(_)) {
                apibail_generic!("private route must have first hop");
            }
        }

        cache.add_remote_route(
            cur_ts,
            RemoteRouteSetId::from_route_id(id.clone()),
            private_routes,
        );

        Ok(id)
    }

    /// Add a single remote private route for compilation
    /// It is safe to add the same route more than once and it will return the same route id
    /// Returns a route set id
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub fn import_single_remote_route(
        &self,
        private_route: Arc<PrivateRoute>,
    ) -> VeilidAPIResult<RouteId> {
        let cur_ts = Timestamp::now();

        // Make a single route set
        let private_routes = vec![private_route];

        // make the route id
        let id = self.generate_remote_route_id(&private_routes)?;

        // validate the private routes
        for private_route in &private_routes {
            // ensure private route has first hop
            if !matches!(private_route.hops, PrivateRouteHops::FirstHop(_)) {
                apibail_generic!("private route must have first hop");
            }
        }

        self.cache.write().add_remote_route(
            cur_ts,
            RemoteRouteSetId::from_route_id(id.clone()),
            private_routes,
        );

        Ok(id)
    }

    /// Release a remote private route that is no longer in use
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub(super) fn release_remote_route_id(&self, id: RemoteRouteSetId) -> bool {
        self.cache.write().remove_remote_route(id)
    }

    /// Get a route id for a route's public key
    pub fn get_route_id_for_key(&self, key: &PublicKey) -> Option<RouteId> {
        // Careful with locking order here, we need to lock the content before the cache
        let content = self.content.read();
        let cache = self.cache.read();

        // Check for allocated route
        if let Some(id) = content.get_id_by_key(key) {
            return Some(id.into());
        }

        // Check for remote route
        if let Some(rrid) = cache.get_remote_route_id_by_key(key) {
            return Some(rrid.into());
        }

        None
    }

    /// Check to see if this remote (not ours) private route has seen our current node info yet
    /// This happens when you communicate with a private route without a safety route
    pub fn has_remote_private_route_seen_our_node_info(
        &self,
        key: &PublicKey,
        published_peer_info: &PeerInfo,
    ) -> bool {
        let cache = self.cache.read();

        // Check for allocated route. If this is not a remote private route,
        // we may be running a test and using our own allocated route as the destination private route.
        // In that case we definitely have already seen our own node info
        if cache.get_allocated_route_id_by_key(key).is_some() {
            return true;
        }

        if let Some(rrid) = cache.get_remote_route_id_by_key(key) {
            let cur_ts = Timestamp::now();
            if let Some(rrce) = cache.peek_remote_route(cur_ts, &rrid) {
                return rrce.has_seen_our_node_info_ts(published_peer_info.node_info().timestamp());
            }
        }

        false
    }

    /// Convert binary blob to private route vector
    fn blob_to_private_routes(&self, blob: Vec<u8>) -> VeilidAPIResult<Vec<Arc<PrivateRoute>>> {
        // Deserialize count
        if blob.is_empty() {
            apibail_invalid_argument!("not deserializing empty private route blob", "blob", &blob);
        }

        let pr_count = blob[0] as usize;
        if pr_count > MAX_CRYPTO_KINDS {
            apibail_invalid_argument!("too many crypto kinds to decode blob", "blob[0]", pr_count);
        }

        // Deserialize stream of private routes
        let decode_context = RPCDecodeContext {
            registry: self.registry(),
            origin_routing_domain: RoutingDomain::PublicInternet,
        };
        let pr_slice = &blob[1..];
        let mut pr_cursor = std::io::Cursor::new(pr_slice);
        let mut out = Vec::with_capacity(pr_count);
        for _ in 0..pr_count {
            let reader = capnp::serialize_packed::read_message(
                &mut pr_cursor,
                capnp::message::ReaderOptions::new(),
            )
            .map_err(|e| {
                VeilidAPIError::parse_error(format!("failed to read blob: {}", e), &blob)
            })?;

            let pr_reader = reader
                .get_root::<veilid_capnp::private_route::Reader>()
                .map_err(VeilidAPIError::internal)?;
            let private_route = decode_private_route(&decode_context, &pr_reader).map_err(|e| {
                VeilidAPIError::parse_error(format!("failed to decode private route: {}", e), &blob)
            })?;

            out.push(Arc::new(private_route));
        }

        // Don't trust the order of the blob
        out.sort_unstable_by(|a, b| a.public_key.cmp(&b.public_key));

        Ok(out)
    }

    /// Generate RouteId from set of private routes
    fn generate_remote_route_id(
        &self,
        private_routes: &[Arc<PrivateRoute>],
    ) -> VeilidAPIResult<RouteId> {
        let crypto = self.crypto();

        let pkbyteslen = private_routes
            .iter()
            .fold(0, |acc, x| acc + x.public_key.ref_value().len());
        let mut pkbytes = Vec::with_capacity(pkbyteslen);
        let mut best_kind: Option<CryptoKind> = None;
        for private_route in private_routes {
            if best_kind.is_none()
                || compare_crypto_kind(
                    &private_route.public_key.kind(),
                    best_kind.as_ref().unwrap_or_log(),
                ) == cmp::Ordering::Less
            {
                best_kind = Some(private_route.public_key.kind());
            }
            pkbytes.extend_from_slice(private_route.public_key.ref_value());
        }
        let Some(best_kind) = best_kind else {
            apibail_internal!("no compatible crypto kinds in route");
        };
        let vcrypto = crypto.get(best_kind).unwrap_or_log();

        Ok(RouteId::new(
            vcrypto.kind(),
            BareRouteId::new(vcrypto.generate_hash(&pkbytes).ref_value()),
        ))
    }
}
