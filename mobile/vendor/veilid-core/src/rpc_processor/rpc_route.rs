use super::*;

impl_veilid_log_facility!("rpc");

impl RPCProcessor {
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn process_route_safety_route_hop(
        &self,
        routed_operation: RoutedOperation,
        route_hop: RouteHop,
        safety_route: Arc<SafetyRoute>,
        route_op_id: OperationId,
    ) -> RPCNetworkResult<()> {
        // Get next hop node ref
        let routing_table = self.routing_table();
        let Some(next_hop_nr) = route_hop.node.node_ref(&routing_table) else {
            return Ok(NetworkResult::invalid_message(format!(
                "could not get route node hop ref: {}",
                route_hop.node.describe()
            )));
        };

        // Apply sequencing preference and require routing domain to remain the same (for now)
        let next_hop_nr = next_hop_nr
            .sequencing_filtered(routed_operation.sequencing())
            .with_routing_domain(RoutingDomain::PublicInternet);

        // Pass along the route
        let next_hop_route = RPCOperationRoute::new(
            Arc::new(SafetyRoute {
                public_key: safety_route.public_key.clone(),
                hops: SafetyRouteHops::Data(route_hop.next_hop.unwrap_or_log()),
            }),
            routed_operation,
        );
        let next_hop_route_stmt =
            RPCStatement::new(RPCStatementDetail::Route(Box::new(next_hop_route)));

        // Send the next route statement
        Box::pin(
            self.statement(
                Destination::direct(next_hop_nr, None),
                next_hop_route_stmt,
                None,
                Some(route_op_id),
            )
            .measure_debug(TimestampDuration::new_ms(200), |dur| {
                veilid_log!(self debug "process_route_safety_route_hop rpc_route stmt: {:#}", dur);
            }),
        )
        .await
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn process_route_private_route_hop(
        &self,
        routed_operation: RoutedOperation,
        next_route_node: RouteNode,
        safety_route_public_key: PublicKey,
        next_private_route: Arc<PrivateRoute>,
        route_op_id: OperationId,
    ) -> RPCNetworkResult<()> {
        // Get next hop node ref
        let routing_table = self.routing_table();
        let Some(next_hop_nr) = next_route_node.node_ref(&routing_table) else {
            return Ok(NetworkResult::invalid_message(format!(
                "could not get route node hop ref: {}",
                next_route_node.describe()
            )));
        };

        // Apply sequencing preference and require routing domain to be PublicInternet for now
        let next_hop_nr = next_hop_nr
            .sequencing_filtered(routed_operation.sequencing())
            .with_routing_domain(RoutingDomain::PublicInternet);

        // Pass along the route
        let next_hop_route = RPCOperationRoute::new(
            Arc::new(SafetyRoute {
                public_key: safety_route_public_key,
                hops: SafetyRouteHops::Private(next_private_route),
            }),
            routed_operation,
        );
        let next_hop_route_stmt =
            RPCStatement::new(RPCStatementDetail::Route(Box::new(next_hop_route)));

        // Send the next route statement
        Box::pin(
            self.statement(
                Destination::direct(next_hop_nr, None),
                next_hop_route_stmt,
                None,
                Some(route_op_id),
            )
            .measure_debug(TimestampDuration::new_ms(200), |dur| {
                veilid_log!(self debug "process_route_private_route_hop rpc_route stmt: {:#}", dur);
            }),
        )
        .await
    }

    /// Process a routed operation that came in over a safety route but no private route
    ///
    /// Note: it is important that we never respond with a safety route to questions that come
    /// in without a private route. Giving away a safety route when the node id is known is
    /// a privacy violation!
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn process_safety_routed_operation(
        &self,
        detail: RPCMessageHeaderDetailDirect,
        vcrypto: &AsyncCryptoSystemGuard<'_>,
        routed_operation: RoutedOperation,
        remote_sr_pubkey: PublicKey,
        route_op_id: OperationId,
    ) -> RPCNetworkResult<()> {
        // Now that things are valid, decrypt the routed operation with DEC(nonce, DH(the SR's public key, the PR's (or node's) secret)
        // xxx: punish nodes that send messages that fail to decrypt eventually? How to do this for safety routes?
        let secret_key = self.routing_table().secret_key(remote_sr_pubkey.kind());
        let Ok(dh_secret) = vcrypto.cached_dh(&remote_sr_pubkey, &secret_key).await else {
            return Ok(NetworkResult::invalid_message(
                "dh failed for remote safety route for safety routed operation",
            ));
        };
        let body = match vcrypto
            .decrypt_aead(
                routed_operation.data(),
                routed_operation.nonce(),
                &dh_secret,
                None,
            )
            .await
        {
            Ok(v) => v,
            Err(e) => {
                return Ok(NetworkResult::invalid_message(format!(
                    "decryption of routed operation failed: {}",
                    e
                )));
            }
        };

        // Pass message to RPC system
        self.enqueue_safety_routed_message(
            detail,
            remote_sr_pubkey,
            routed_operation.sequencing(),
            route_op_id,
            body,
        )
        .map_err(RPCError::internal)?;

        Ok(NetworkResult::value(()))
    }

    /// Process a routed operation that came in over both a safety route and a private route
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn process_private_routed_operation(
        &self,
        detail: RPCMessageHeaderDetailDirect,
        vcrypto: &AsyncCryptoSystemGuard<'_>,
        routed_operation: RoutedOperation,
        remote_sr_pubkey: PublicKey,
        pr_pubkey: PublicKey,
        route_op_id: OperationId,
    ) -> RPCNetworkResult<()> {
        // Get sender id of the peer with the crypto kind of the route
        let Some(sender_id) = detail.sender_noderef.node_ids().get(pr_pubkey.kind()) else {
            return Ok(NetworkResult::invalid_message(
                "route node doesnt have a required crypto kind for routed operation",
            ));
        };

        // Look up the private route and ensure it's one in our spec store
        // Ensure the route is validated, and construct a return safetyspec that matches the inbound preferences
        let routing_table = self.routing_table();
        let rss = routing_table.route_spec_store();

        let Some((secret_key, safety_spec)) = rss
            .get_signature_validated_route(
                &pr_pubkey,
                routed_operation.signatures(),
                routed_operation.data(),
                routed_operation.sequencing(),
                &sender_id,
            )
            .await
        else {
            return Ok(NetworkResult::invalid_message(
                "signatures did not validate for private route",
            ));
        };

        // Now that things are valid, decrypt the routed operation with DEC(nonce, DH(the SR's public key, the PR's (or node's) secret)
        // xxx: punish nodes that send messages that fail to decrypt eventually. How to do this for private routes?
        let Ok(dh_secret) = vcrypto.cached_dh(&remote_sr_pubkey, &secret_key).await else {
            return Ok(NetworkResult::invalid_message(
                "dh failed for remote safety route for private routed operation",
            ));
        };
        let Ok(body) = vcrypto
            .decrypt_aead(
                routed_operation.data(),
                routed_operation.nonce(),
                &dh_secret,
                None,
            )
            .await
        else {
            return Ok(NetworkResult::invalid_message(
                "decryption of routed operation failed",
            ));
        };

        // Pass message to RPC system
        self.enqueue_private_routed_message(
            detail,
            remote_sr_pubkey,
            pr_pubkey,
            safety_spec,
            route_op_id,
            body,
        )
        .map_err(RPCError::internal)?;

        Ok(NetworkResult::value(()))
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn process_routed_operation(
        &self,
        detail: RPCMessageHeaderDetailDirect,
        vcrypto: &AsyncCryptoSystemGuard<'_>,
        routed_operation: RoutedOperation,
        remote_sr_pubkey: PublicKey,
        pr_pubkey: PublicKey,
        route_op_id: OperationId,
    ) -> RPCNetworkResult<()> {
        // If the private route public key is our public key, then this was sent via safety route to our node directly
        // so there will be no signatures to validate
        if self.routing_table().public_keys().contains(&pr_pubkey) {
            // The private route was a stub
            self.process_safety_routed_operation(
                detail,
                vcrypto,
                routed_operation,
                remote_sr_pubkey,
                route_op_id,
            )
            .await
        } else {
            // Both safety and private routes used, should reply with a safety route
            self.process_private_routed_operation(
                detail,
                vcrypto,
                routed_operation,
                remote_sr_pubkey,
                pr_pubkey,
                route_op_id,
            )
            .await
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn process_private_route_first_hop(
        &self,
        mut routed_operation: RoutedOperation,
        sr_pubkey: PublicKey,
        pr_first_hop: RouteNode,
        private_route: PrivateRoute,
        route_op_id: OperationId,
    ) -> RPCNetworkResult<()> {
        // Check for loopback test where private route is the same as safety route
        if sr_pubkey == private_route.public_key {
            // If so, we're going to turn this thing right around without transiting the network
            let PrivateRouteHops::Data(route_hop_data) = private_route.hops else {
                return Ok(NetworkResult::invalid_message(
                    "Loopback test requires hops",
                ));
            };

            // Decrypt route hop data
            let route_hop = network_result_try!(
                self.decrypt_private_route_hop_data(
                    &route_hop_data,
                    &private_route.public_key,
                    &mut routed_operation
                )
                .await?
            );

            // Make next PrivateRoute and pass it on
            return self
                .process_route_private_route_hop(
                    routed_operation,
                    route_hop.node,
                    sr_pubkey,
                    Arc::new(PrivateRoute {
                        public_key: private_route.public_key,
                        hops: route_hop
                            .next_hop
                            .map(PrivateRouteHops::Data)
                            .unwrap_or(PrivateRouteHops::Empty),
                    }),
                    route_op_id,
                )
                .await;
        }

        // Switching to private route from safety route
        self.process_route_private_route_hop(
            routed_operation,
            pr_first_hop,
            sr_pubkey,
            Arc::new(private_route),
            route_op_id,
        )
        .await
    }

    /// Decrypt route hop data and sign routed operation
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn decrypt_private_route_hop_data(
        &self,
        route_hop_data: &RouteHopData,
        pr_pubkey: &PublicKey,
        routed_operation: &mut RoutedOperation,
    ) -> RPCNetworkResult<RouteHop> {
        // Get crypto kind
        let crypto = self.crypto();
        let crypto_kind = pr_pubkey.kind();
        let Some(vcrypto) = crypto.get_async(crypto_kind) else {
            return Ok(NetworkResult::invalid_message(
                "private route hop data crypto is not supported",
            ));
        };

        // Decrypt the blob with DEC(nonce, DH(the PR's public key, this hop's secret)
        let secret_key = self.routing_table().secret_key(crypto_kind);
        let dh_secret = vcrypto
            .cached_dh(pr_pubkey, &secret_key)
            .await
            .map_err(RPCError::protocol)?;
        let dec_blob_data = match vcrypto
            .decrypt_aead(
                route_hop_data.blob.clone(),
                &route_hop_data.nonce,
                &dh_secret,
                None,
            )
            .await
        {
            Ok(v) => v,
            Err(e) => {
                return Ok(NetworkResult::invalid_message(format!(
                    "unable to decrypt private route hop data: {}",
                    e
                )));
            }
        };
        let dec_blob_reader = MessageData::new(dec_blob_data).get_reader()?;

        // Decode next RouteHop
        let route_hop = {
            let rh_reader = dec_blob_reader.get_root::<veilid_capnp::route_hop::Reader>()?;
            let decode_context = RPCDecodeContext {
                registry: self.registry(),
                origin_routing_domain: routed_operation.routing_domain(),
            };
            decode_route_hop(&decode_context, &rh_reader)?
        };

        // Sign the operation if this is not our last hop
        // as the last hop is already signed by the envelope
        if route_hop.next_hop.is_some() {
            let public_key = self.routing_table().public_key(crypto_kind);
            let secret_key = self.routing_table().secret_key(crypto_kind);
            let sig = vcrypto
                .sign(&public_key, &secret_key, routed_operation.data())
                .await
                .map_err(RPCError::internal)?;
            if let Err(e) = routed_operation.add_signature(sig) {
                return Ok(NetworkResult::invalid_message(format!(
                    "too many signatures: {}",
                    e
                )));
            }
        }

        Ok(NetworkResult::value(route_hop))
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rpc", skip(self), ret, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub(super) async fn process_route(&self, msg: Message) -> RPCNetworkResult<()> {
        // Ignore if disabled
        let routing_table = self.routing_table();
        let crypto = self.crypto();

        let Some(published_peer_info) =
            routing_table.get_published_peer_info(msg.header.routing_domain())
        else {
            return Ok(NetworkResult::service_unavailable(
                "Own node info must be published to route",
            ));
        };

        // Get header detail, must be direct and not inside a route itself
        let detail = match msg.header.detail {
            RPCMessageHeaderDetail::Direct(detail) => detail,
            RPCMessageHeaderDetail::SafetyRouted(_) | RPCMessageHeaderDetail::PrivateRouted(_) => {
                return Ok(NetworkResult::invalid_message(
                    "route operation can not be inside route",
                ))
            }
        };

        // Get the statement
        let (route_op_id, _, kind) = msg.operation.destructure();
        let route = match kind {
            RPCOperationKind::Statement(s) => match s.destructure() {
                RPCStatementDetail::Route(s) => s,
                _ => return Err(RPCError::internal("not a route statement")),
            },
            _ => return Err(RPCError::internal("not a statement")),
        };

        // Get crypto kind
        let (safety_route, mut routed_operation) = route.destructure();

        let crypto_kind = safety_route.crypto_kind();
        let Some(vcrypto) = crypto.get_async(crypto_kind) else {
            return Ok(NetworkResult::invalid_message(
                "routed operation crypto is not supported",
            ));
        };

        // See what kind of safety route we have going on here
        match &safety_route.hops {
            // There is a safety route hop
            SafetyRouteHops::Data(route_hop_data) => {
                // Don't accept route since it's not the last hop
                if !published_peer_info
                    .node_info()
                    .has_capability(VEILID_CAPABILITY_ROUTE)
                {
                    return Ok(NetworkResult::service_unavailable("route is not available"));
                }

                // Decrypt the blob with DEC(nonce, DH(the SR's public key, this hop's secret)
                let secret_key = self.routing_table().secret_key(crypto_kind);
                let Ok(dh_secret) = vcrypto
                    .cached_dh(&safety_route.public_key, &secret_key)
                    .await
                else {
                    return Ok(NetworkResult::invalid_message(
                        "dh failed for safety route hop",
                    ));
                };
                let Ok(dec_blob_data) = vcrypto
                    .decrypt_aead(
                        route_hop_data.blob.clone(),
                        &route_hop_data.nonce,
                        &dh_secret,
                        None,
                    )
                    .await
                else {
                    return Ok(NetworkResult::invalid_message(
                        "failed to decrypt route hop data for safety route hop",
                    ));
                };

                // See if this is last hop in safety route, if so, we're decoding a PrivateRoute not a RouteHop
                let Some(dec_blob_tag) = dec_blob_data.last().copied() else {
                    return Ok(NetworkResult::invalid_message("no bytes in blob"));
                };

                let Ok(dec_blob_reader) =
                    MessageData::new(dec_blob_data.slice(..dec_blob_data.len() - 1)).get_reader()
                else {
                    return Ok(NetworkResult::invalid_message(
                        "Failed to decode RPCMessageData from blob",
                    ));
                };

                // Decode the blob appropriately
                if dec_blob_tag == 1 {
                    // PrivateRoute
                    let private_route = {
                        let Ok(pr_reader) =
                            dec_blob_reader.get_root::<veilid_capnp::private_route::Reader>()
                        else {
                            return Ok(NetworkResult::invalid_message(
                                "failed to get private route reader for blob",
                            ));
                        };
                        let decode_context = RPCDecodeContext {
                            registry: self.registry(),
                            origin_routing_domain: routed_operation.routing_domain(),
                        };
                        let Ok(private_route) = decode_private_route(&decode_context, &pr_reader)
                        else {
                            return Ok(NetworkResult::invalid_message(
                                "failed to decode private route",
                            ));
                        };
                        private_route
                    };

                    // Split off the first hop
                    let (opt_pr_first_hop_node, private_route) = private_route.split_first_hop();
                    let Some(pr_first_hop_node) = opt_pr_first_hop_node else {
                        return Ok(NetworkResult::invalid_message(
                            "switching from safety route to private route requires first hop",
                        ));
                    };

                    // Switching from full safety route to private route first hop
                    network_result_try!(
                        self.process_private_route_first_hop(
                            routed_operation,
                            safety_route.public_key.clone(),
                            pr_first_hop_node,
                            private_route,
                            route_op_id,
                        )
                        .await?
                    );
                } else if dec_blob_tag == 0 {
                    // RouteHop
                    let route_hop = {
                        let Ok(rh_reader) =
                            dec_blob_reader.get_root::<veilid_capnp::route_hop::Reader>()
                        else {
                            return Ok(NetworkResult::invalid_message(
                                "failed to get route hop reader for blob",
                            ));
                        };
                        let decode_context = RPCDecodeContext {
                            registry: self.registry(),
                            origin_routing_domain: routed_operation.routing_domain(),
                        };
                        let Ok(route_hop) = decode_route_hop(&decode_context, &rh_reader) else {
                            return Ok(NetworkResult::invalid_message(
                                "failed to decode route hop",
                            ));
                        };
                        route_hop
                    };

                    // Continue the full safety route with another hop
                    network_result_try!(
                        self.process_route_safety_route_hop(
                            routed_operation,
                            route_hop,
                            safety_route,
                            route_op_id,
                        )
                        .await?
                    );
                } else {
                    return Ok(NetworkResult::invalid_message("invalid blob tag"));
                }
            }
            // No safety route left, now doing private route
            SafetyRouteHops::Private(private_route) => {
                // See if we have a hop, if not, we are at the end of the private route
                match &private_route.hops {
                    PrivateRouteHops::FirstHop(_) => {
                        // Don't accept route since it's not the last hop
                        if !published_peer_info
                            .node_info()
                            .has_capability(VEILID_CAPABILITY_ROUTE)
                        {
                            return Ok(NetworkResult::service_unavailable(
                                "route is not available",
                            ));
                        }

                        // Safety route was a stub, start with the beginning of the private route
                        let (opt_pr_first_hop_node, private_route) =
                            private_route.split_first_hop();
                        let Some(pr_first_hop_node) = opt_pr_first_hop_node else {
                            return Err(RPCError::internal("must have had first hop"));
                        };
                        network_result_try!(
                            self.process_private_route_first_hop(
                                routed_operation,
                                safety_route.public_key.clone(),
                                pr_first_hop_node,
                                private_route,
                                route_op_id,
                            )
                            .await?
                        );
                    }
                    PrivateRouteHops::Data(route_hop_data) => {
                        // Don't accept route since it's not the last hop
                        if !published_peer_info
                            .node_info()
                            .has_capability(VEILID_CAPABILITY_ROUTE)
                        {
                            return Ok(NetworkResult::service_unavailable(
                                "route is not available",
                            ));
                        }

                        // Decrypt route hop data
                        let route_hop = network_result_try!(
                            self.decrypt_private_route_hop_data(
                                route_hop_data,
                                &private_route.public_key,
                                &mut routed_operation
                            )
                            .await?
                        );

                        // Make next PrivateRoute and pass it on
                        network_result_try!(
                            self.process_route_private_route_hop(
                                routed_operation,
                                route_hop.node,
                                safety_route.public_key.clone(),
                                Arc::new(PrivateRoute {
                                    public_key: private_route.public_key.clone(),
                                    hops: route_hop
                                        .next_hop
                                        .map(PrivateRouteHops::Data)
                                        .unwrap_or(PrivateRouteHops::Empty),
                                }),
                                route_op_id,
                            )
                            .await?
                        );
                    }
                    PrivateRouteHops::Empty => {
                        // No hops left, time to process the routed operation
                        network_result_try!(
                            self.process_routed_operation(
                                detail,
                                &vcrypto,
                                routed_operation,
                                safety_route.public_key.clone(),
                                private_route.public_key.clone(),
                                route_op_id,
                            )
                            .await?
                        );
                    }
                }
            }
        }

        Ok(NetworkResult::value(()))
    }
}
