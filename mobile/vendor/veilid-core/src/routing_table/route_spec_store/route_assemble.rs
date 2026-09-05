use super::*;

impl RouteSpecStore {
    /// Assemble a single private route for publication from an allocated route key
    /// Returns a PrivateRoute object for an allocated route key
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn assemble_single_private_route(
        &self,
        allocated_route_key: &PublicKey,
        optimized: Option<bool>,
    ) -> VeilidAPIResult<Arc<PrivateRoute>> {
        let (key, secret_key, hop_node_refs, optimized) = {
            let cache = self.cache.read();
            let Some(id) = cache.get_allocated_route_id_by_key(allocated_route_key) else {
                // Route doesn't exist
                apibail_invalid_target!("route key does not exist");
            };
            let Some(arce) = cache.get_allocated_route_by_id(&id) else {
                apibail_internal!("route id does not exist");
            };

            // See if we can optimize this compilation yet
            // We don't want to include full nodeinfo if we don't have to
            let optimized =
                optimized.unwrap_or(arce.with_stats(|stats| stats.last_known_valid_ts().is_some()));

            let secret_key = arce.route_set_secret_for_key(allocated_route_key)?;

            (
                allocated_route_key.clone(),
                secret_key,
                arce.hop_node_refs(),
                optimized,
            )
        };

        let routing_table = self.routing_table();
        Self::assemble_single_private_route_inner(
            &routing_table,
            &key,
            &secret_key,
            &hop_node_refs,
            optimized,
        )
        .await
    }

    /// Assemble private route set for publication
    /// Returns a vec of assembled PrivateRoute objects for an RouteId
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn assemble_private_route_set(
        &self,
        id: &AllocatedRouteSetId,
        optimized: Option<bool>,
    ) -> VeilidAPIResult<Vec<Arc<PrivateRoute>>> {
        let route_params = {
            let cache = self.cache.read();
            let Some(arce) = cache.get_allocated_route_by_id(id) else {
                apibail_invalid_target!("route id does not exist");
            };

            // See if we can optimize this compilation yet
            // We don't want to include full nodeinfo if we don't have to
            let optimized =
                optimized.unwrap_or(arce.with_stats(|stats| stats.last_known_valid_ts().is_some()));

            let mut route_params = Vec::new();

            for key in arce.route_set_keys().iter() {
                let secret_key = arce.route_set_secret_for_key(key)?;
                let hop_node_refs = arce.hop_node_refs();

                route_params.push((key.clone(), secret_key, hop_node_refs, optimized));
            }

            route_params
        };

        let mut out = Vec::new();
        let routing_table = self.routing_table();
        for (key, secret_key, hop_node_refs, optimized) in route_params {
            out.push(
                Self::assemble_single_private_route_inner(
                    &routing_table,
                    &key,
                    &secret_key,
                    &hop_node_refs,
                    optimized,
                )
                .await?,
            );
        }
        Ok(out)
    }

    pub(super) async fn assemble_single_private_route_inner(
        routing_table: &RoutingTable,
        key: &PublicKey,
        secret_key: &SecretKey,
        hop_node_refs: &[NodeRef],
        optimized: bool,
    ) -> VeilidAPIResult<Arc<PrivateRoute>> {
        // Ensure we get the crypto for it
        let crypto = routing_table.crypto();
        let crypto_kind = key.kind();
        let Some(vcrypto) = crypto.get_async(crypto_kind) else {
            apibail_invalid_argument!("crypto not supported for route", "crypto_kind", crypto_kind);
        };

        let (hop_info, mut route_hop) = {
            // Ensure our network class is valid before attempting to assemble any routes
            let Some(published_peer_info) =
                routing_table.get_published_peer_info(RoutingDomain::PublicInternet)
            else {
                apibail_try_again!("unable to assemble route until we have published peerinfo");
            };

            // Make innermost route hop to our own node
            let route_hop = RouteHop {
                node: if optimized {
                    let Some(node_id) = published_peer_info.node_ids().get(crypto_kind) else {
                        apibail_invalid_argument!(
                            "missing node id for crypto kind",
                            "crypto_kind",
                            crypto_kind
                        );
                    };
                    RouteNode::NodeId(node_id)
                } else {
                    RouteNode::PeerInfo(published_peer_info)
                },
                next_hop: None,
            };

            // Iterate hops in private route order (reverse, but inside out)
            let mut hop_info = Vec::with_capacity(hop_node_refs.len());
            for hop_node_ref in hop_node_refs {
                let Some(hop_node_id) = hop_node_ref.node_ids().get(crypto_kind) else {
                    apibail_invalid_argument!(
                        "no hop node id for route hop",
                        "crypto_kind",
                        crypto_kind
                    );
                };
                let Some(hop_public_key) = hop_node_ref
                    .public_keys(RoutingDomain::PublicInternet)
                    .get(crypto_kind)
                else {
                    apibail_invalid_argument!(
                        "no hop public key for route hop",
                        "crypto_kind",
                        crypto_kind
                    );
                };
                let Some(hop_peer_info) = hop_node_ref.get_peer_info(RoutingDomain::PublicInternet)
                else {
                    apibail_invalid_argument!(
                        "no hop peer info for route hop",
                        "crypto_kind",
                        crypto_kind
                    );
                };
                hop_info.push((hop_node_id, hop_public_key, hop_peer_info));
            }

            (hop_info, route_hop)
        };

        let num_hops = hop_info.len();
        for (i, (hop_node_id, hop_public_key, hop_peer_info)) in hop_info.into_iter().enumerate() {
            // Encrypt the previous blob ENC(nonce, DH(PKhop,SKpr))
            let nonce = vcrypto.random_nonce().await;

            let blob_data = {
                let mut rh_message = ::capnp::message::Builder::new_default();
                let mut rh_builder = rh_message.init_root::<veilid_capnp::route_hop::Builder>();
                encode_route_hop(&route_hop, &mut rh_builder)?;
                canonical_message_builder_to_bytes_writer_packed(rh_message, |size| {
                    BytesWriter::with_capacity(size + vcrypto.aead_overhead())
                })?
                .into_inner()
            };

            let dh_secret = vcrypto.cached_dh(&hop_public_key, secret_key).await?;
            let enc_msg_data = vcrypto
                .encrypt_in_place_aead(blob_data, &nonce, &dh_secret, None)
                .await?;
            let route_hop_data = RouteHopData {
                nonce,
                blob: enc_msg_data.into(),
            };

            // The entry hop is resolved by the recipient, who may never have
            // seen it; NodeId-only there black-holes replies. Temporary until
            // PR2.0 removes route optimization entirely.
            let is_entry_hop = i == num_hops - 1;
            route_hop = RouteHop {
                node: if optimized && !is_entry_hop {
                    // Optimized, no peer info, just the node id
                    RouteNode::NodeId(hop_node_id)
                } else {
                    RouteNode::PeerInfo(hop_peer_info)
                },
                next_hop: Some(route_hop_data),
            }
        }

        let private_route = Arc::new(PrivateRoute {
            public_key: key.clone(),
            hops: PrivateRouteHops::FirstHop(Box::new(route_hop)),
        });
        Ok(private_route)
    }
}
