use super::*;

#[derive(Clone, Debug)]
pub struct CompileRouteParams {
    pub safety_selection: SafetySelection,
    pub private_route: Arc<PrivateRoute>,
    pub opt_reply_pr_pubkey: Option<PublicKey>, // Should be route id someday
}
// Compiled route key for caching
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CompiledRouteCacheKey {
    pub safety_selection: SafetySelection,
    pub pr_pubkey: PublicKey,                   // Should be route id someday
    pub opt_reply_pr_pubkey: Option<PublicKey>, // Should be route id someday
}

impl From<&CompileRouteParams> for CompiledRouteCacheKey {
    fn from(params: &CompileRouteParams) -> Self {
        Self {
            safety_selection: params.safety_selection.clone(),
            pr_pubkey: params.private_route.public_key.clone(),
            opt_reply_pr_pubkey: params.opt_reply_pr_pubkey.clone(),
        }
    }
}

/// Compiled route (safety route + private route)
#[derive(Debug)]
pub struct CompiledRoute {
    /// The safety route attached to the private route
    pub compiled_safety_route: Arc<SafetyRoute>,
    /// Whether or not the private route was allocated (false for stubs, imported remote routes, or non-imported remote routes)
    pub pr_allocated: bool,
    /// The secret used to encrypt the message payload
    pub secret: SecretKey,
    /// The node ref to the first hop in the compiled route
    /// filtered to the safetyselection it was compiled with
    pub first_hop: FilteredNodeRef,
    /// Whether this compiled route was built with optimized hop encoding (NodeId only vs full PeerInfo)
    pub optimized: bool,
}

/// The state of a cache lookup for a compiled route
pub enum RouteCompileCacheDisposition {
    /// The route needs to be compiled and re-cached; carries the locked cache entry
    Compile(CompiledRouteCacheEntry),
    /// The route needs to be optimized and re-cached; carries the locked cache entry
    Optimize(CompiledRouteCacheEntry),
    /// The route exists in the cache and can be used as-is
    Cached(Arc<CompiledRoute>),
}

/// Internal parameters for building route hops, shared between optimized and unoptimized compilation
struct RouteCompileContext {
    /// The public key of the safety route
    sr_pubkey: PublicKey,
    /// The first hop in the compiled route filtered to the safety selection it was compiled with
    first_hop: FilteredNodeRef,
    /// The secret for the safety route
    secret: SecretKey,
    /// The node refs of the hops in the safety route
    hop_node_refs: Vec<NodeRef>,
    /// Whether this compiled route can be built with optimized hop encoding (NodeId only vs full PeerInfo)
    optimize: bool,
    /// Whether or not the private route was allocated
    pr_allocated: bool,
}

impl RouteSpecStore {
    /// Extracts the compile context from route spec details for a resolved safety route.
    fn get_route_compile_context(
        &self,
        safety_route_id: &AllocatedRouteSetId,
        sr_pubkey: PublicKey,
        safety_spec: &SafetySpec,
        optimize: bool,
        pr_allocated: bool,
    ) -> VeilidAPIResult<RouteCompileContext> {
        let cache = self.cache.read();

        let Some(safety_arce) = cache.get_allocated_route_by_id(safety_route_id) else {
            apibail_internal!("safety route not in cache");
        };

        // Get the first hop noderef of the safety route
        let first_hop = safety_arce.hop_node_ref(0).unwrap_or_log();

        // Ensure sequencing requirement is set on first hop
        let mut first_hop = first_hop.sequencing_filtered(safety_spec.sequencing);

        // Enforce the routing domain
        first_hop
            .merge_filter(NodeRefFilter::new().with_routing_domain(RoutingDomain::PublicInternet));

        let secret = safety_arce.route_set_secret_for_key(&sr_pubkey)?;

        let hop_node_refs = safety_arce.hop_node_refs();

        Ok(RouteCompileContext {
            sr_pubkey,
            first_hop,
            secret,
            hop_node_refs,
            optimize,
            pr_allocated,
        })
    }

    /// Build the encrypted hop chain
    async fn compile_route_inner(
        routing_table: &RoutingTable,
        params: &CompileRouteParams,
        ctx: RouteCompileContext,
    ) -> VeilidAPIResult<Arc<CompiledRoute>> {
        let crypto_kind = params.private_route.crypto_kind();
        let crypto = routing_table.crypto();
        let Some(vcrypto) = crypto.get_async(crypto_kind) else {
            apibail_generic!("crypto not supported for route");
        };

        // start last blob-to-encrypt data off as private route
        let mut blob_data = {
            let mut pr_message = ::capnp::message::Builder::new_default();
            let mut pr_builder = pr_message.init_root::<veilid_capnp::private_route::Builder>();
            encode_private_route(&params.private_route, &mut pr_builder)?;
            let mut blob_data =
                canonical_message_builder_to_bytes_writer_packed(pr_message, |size| {
                    BytesWriter::with_capacity(size + 1 + vcrypto.aead_overhead())
                })?
                .into_inner();

            // append the private route tag so we know how to decode it later
            blob_data.extend_from_slice(&[1u8]);
            blob_data
        };

        // Encode each hop from inside to outside
        // skips the outermost hop since that's entering the
        // safety route and does not include the dialInfo
        // (outer hop is a RouteHopData, not a RouteHop).
        // Each loop mutates 'nonce', and 'blob_data'
        let mut nonce = vcrypto.random_nonce().await;

        let mut hop_info = Vec::with_capacity(ctx.hop_node_refs.len() - 1);
        let first_hop_public_key = {
            // Forward order (safety route), but inside-out
            for h in (1..ctx.hop_node_refs.len()).rev() {
                let hop_node_ref = &ctx.hop_node_refs[h];
                let (hop_node_id, hop_public_key, hop_peer_info) = {
                    hop_node_ref.operate(|e| {
                        let Some(hop_node_id) = e.node_ids().get(crypto_kind) else {
                            apibail_invalid_argument!(
                                "no hop node id for route hop",
                                "crypto_kind",
                                crypto_kind
                            );
                        };
                        let Some(hop_public_key) = e
                            .public_keys(RoutingDomain::PublicInternet)
                            .get(crypto_kind)
                        else {
                            apibail_invalid_argument!(
                                "no hop public key for route hop",
                                "crypto_kind",
                                crypto_kind
                            );
                        };
                        let Some(hop_peer_info) = e.get_peer_info(RoutingDomain::PublicInternet)
                        else {
                            apibail_invalid_argument!(
                                "no hop peer info for route hop",
                                "crypto_kind",
                                crypto_kind
                            );
                        };
                        Ok((hop_node_id, hop_public_key, hop_peer_info))
                    })?
                };

                hop_info.push((hop_node_id, hop_public_key, hop_peer_info));
            }

            let first_hop_node_ref = &ctx.hop_node_refs[0];
            let Some(first_hop_public_key) = first_hop_node_ref
                .public_keys(RoutingDomain::PublicInternet)
                .get(crypto_kind)
            else {
                apibail_invalid_argument!(
                    "no hop public key for route hop",
                    "crypto_kind",
                    crypto_kind
                );
            };
            first_hop_public_key
        };

        for (hop_node_id, hop_public_key, hop_peer_info) in hop_info {
            // Get blob to encrypt for next hop
            blob_data = {
                // Encrypt the previous blob ENC(nonce, DH(PKhop,SKsr))
                let dh_secret = vcrypto
                    .cached_dh(&hop_public_key, &ctx.secret)
                    .await
                    .map_err(VeilidAPIError::internal)?;
                let enc_blob_data = vcrypto
                    .encrypt_in_place_aead(blob_data, &nonce, &dh_secret, None)
                    .await
                    .map_err(VeilidAPIError::internal)?;

                // Make route hop data
                let route_hop_data = RouteHopData {
                    nonce,
                    blob: enc_blob_data.freeze(),
                };

                // Make route hop
                let route_hop = RouteHop {
                    node: if ctx.optimize {
                        // Optimized, no peer info, just the dht key
                        RouteNode::NodeId(hop_node_id)
                    } else {
                        // Full peer info, required until we are sure the route has been fully established
                        RouteNode::PeerInfo(hop_peer_info)
                    },
                    next_hop: Some(route_hop_data),
                };

                // Make next blob from route hop
                let mut rh_message = ::capnp::message::Builder::new_default();
                let mut rh_builder = rh_message.init_root::<veilid_capnp::route_hop::Builder>();
                encode_route_hop(&route_hop, &mut rh_builder)?;
                let mut blob_data =
                    canonical_message_builder_to_bytes_writer_packed(rh_message, |size| {
                        BytesWriter::with_capacity(size + 1 + vcrypto.aead_overhead())
                    })?
                    .into_inner();

                // Append the route hop tag so we know how to decode it later
                blob_data.extend_from_slice(&[0u8]);
                blob_data
            };

            // Make another nonce for the next hop
            nonce = vcrypto.random_nonce().await;
        }

        // Encode first RouteHopData
        let dh_secret = vcrypto
            .cached_dh(&first_hop_public_key, &ctx.secret)
            .await
            .map_err(VeilidAPIError::internal)?;
        let enc_blob_data = vcrypto
            .encrypt_in_place_aead(blob_data, &nonce, &dh_secret, None)
            .await
            .map_err(VeilidAPIError::internal)?;

        let route_hop_data = RouteHopData {
            nonce,
            blob: enc_blob_data.freeze(),
        };

        let hops = SafetyRouteHops::Data(route_hop_data);

        Ok(Arc::new(CompiledRoute {
            compiled_safety_route: Arc::new(SafetyRoute {
                public_key: ctx.sr_pubkey,
                hops,
            }),
            secret: ctx.secret,
            first_hop: ctx.first_hop,
            optimized: ctx.optimize,
            pr_allocated: ctx.pr_allocated,
        }))
    }

    /// Compiles a safety route to the private route, with caching
    ///
    /// Both optimized and unoptimized routes are cached. When a cached unoptimized
    /// route is found and the safety route has since been validated, the cache entry
    /// is evicted and the route is recompiled as optimized. This ensures that the
    /// same safety route is reused (pinned) across calls while still upgrading to
    /// optimized encoding once the route is proven valid.
    ///
    /// Parameters:
    ///   * safety_selection - The safety selection to use
    ///   * private_route - The private route we are sending to
    ///   * reply_private_route - The private route we want the response to go to, if None, we will se a new safety route
    ///
    /// Returns:
    ///   * Err(VeilidAPIError::TryAgain) if no allocation could happen at this time (not an error)
    ///   * Other Err() if the parameters are wrong
    ///   * Ok(compiled route) on success
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn compile_route(
        &self,
        params: &CompileRouteParams,
    ) -> VeilidAPIResult<Arc<CompiledRoute>> {
        #[cfg(feature = "verbose-tracing")]
        let profile_start_ts = Timestamp::now();

        // Obtain the compile route lock cache lock and determine if we need to compile or optimize
        let (optimize, entry) = {
            let disposition = self.lookup_compiled_route_cache(params).await;
            match disposition {
                RouteCompileCacheDisposition::Compile(entry) => (false, entry),
                RouteCompileCacheDisposition::Optimize(entry) => (true, entry),
                RouteCompileCacheDisposition::Cached(compiled_route) => {
                    #[cfg(feature = "verbose-tracing")]
                    veilid_log!(self trace "compile_route profile (fast path cached): {:#}", Timestamp::now().duration_since(profile_start_ts));
                    return Ok(compiled_route);
                }
            }
        };

        let routing_table = self.routing_table();

        // Get useful private route properties
        let crypto_kind = params.private_route.crypto_kind();
        let pr_pubkey = params.private_route.public_key.clone();
        let opt_private_route_id = {
            // See if this is an allocated private route, if so, get its id
            let cache = self.cache.read();
            cache.get_allocated_route_id_by_key(&pr_pubkey)
        };

        // See if we are using a safety route, if not, short circuit this operation
        let safety_spec = match &params.safety_selection {
            // Safety route spec to use
            SafetySelection::Safe(safety_spec) => safety_spec,
            // Safety route stub with the node's public key as the safety route key since it's the 0th hop
            SafetySelection::Unsafe(sequencing) => {
                let (opt_pr_first_hop_node, private_route) = params.private_route.split_first_hop();
                let Some(pr_first_hop_node) = opt_pr_first_hop_node else {
                    apibail_generic!("compiled private route should have first hop");
                };

                let opt_first_hop = match pr_first_hop_node {
                    RouteNode::NodeId(id) => routing_table
                        .lookup_node_id(id)
                        .map_err(VeilidAPIError::internal)?,
                    RouteNode::PeerInfo(pi) => Some(
                        routing_table
                            .register_node_with_peer_info(pi, false)
                            .map_err(VeilidAPIError::internal)?
                            .unfiltered(),
                    ),
                };
                let Some(first_hop) = opt_first_hop else {
                    // Can't reach this private route any more
                    apibail_generic!("can't reach private route any more");
                };

                // Set sequencing requirement
                let mut first_hop = first_hop.sequencing_filtered(*sequencing);

                // Enforce the routing domain
                first_hop.merge_filter(
                    NodeRefFilter::new().with_routing_domain(RoutingDomain::PublicInternet),
                );

                // Return the compiled safety route
                let stub_compiled_route = Arc::new(CompiledRoute {
                    compiled_safety_route: Arc::new(SafetyRoute::new_stub(
                        routing_table.public_key(crypto_kind),
                        Arc::new(private_route),
                    )),
                    secret: routing_table.secret_key(crypto_kind),
                    first_hop,
                    optimized: true, // Stubs are always considered optimized
                    pr_allocated: opt_private_route_id.is_some(),
                });

                // Cache the compiled route stub
                entry.insert(stub_compiled_route.clone());

                #[cfg(feature = "verbose-tracing")]
                veilid_log!(self debug "compile_route profile (stub): {:#}", Timestamp::now().duration_since(profile_start_ts));
                return Ok(stub_compiled_route);
            }
        };

        // If the safety route requested is also the private route, this is a loopback test, just accept it
        // This means that is a reply private route was specified,
        let opt_safety_route_id_and_public_key = {
            let cache = self.cache.read();
            let opt_private_route_id = cache.get_allocated_route_id_by_key(&pr_pubkey);

            let preferred_route_as_set_id = safety_spec
                .preferred_route
                .as_ref()
                .map(|r| AllocatedRouteSetId::from_route_id(r.clone()));
            if preferred_route_as_set_id.is_some()
                && preferred_route_as_set_id == opt_private_route_id
            {
                // Private route is also safety route during loopback test
                Some((opt_private_route_id.unwrap(), pr_pubkey.clone()))
            } else {
                match params.opt_reply_pr_pubkey.as_ref() {
                    Some(pr_public_key) => {
                        // Symmetric routing: Use the reply private route as the safety route
                        // XXX: add a safety selection switch to use asymmetric routes
                        let Some(reply_private_route_id) =
                            cache.get_allocated_route_id_by_key(pr_public_key)
                        else {
                            // The reply route died since the op began; a retry re-selects
                            apibail_try_again!("safety route id missing, try again later");
                        };
                        Some((reply_private_route_id, pr_public_key.clone()))
                    }
                    None => {
                        // Reply route not specified, choose a safety route
                        None
                    }
                }
            }
        };

        let (safety_route_id, sr_pubkey) =
            if let Some(safety_route_id_and_public_key) = opt_safety_route_id_and_public_key {
                safety_route_id_and_public_key
            } else {
                let Some(avoid_node_id) = params.private_route.first_hop_node_id() else {
                    apibail_generic!("compiled private route should have first hop");
                };
                let select_params = RouteSelectParams {
                    crypto_kind,
                    preferred_route: safety_spec
                        .preferred_route
                        .as_ref()
                        .map(|r| AllocatedRouteSetId::from_route_id(r.clone())),
                    hop_count: safety_spec.hop_count,
                    stability: safety_spec.stability,
                    sequencing: safety_spec.sequencing,
                    directions: Direction::Out.into(),
                    avoid_nodes: vec![avoid_node_id],
                    is_destination_safe: !params.private_route.is_stub(),
                };

                let RouteIdAndKeys {
                    route_id,
                    route_set_keys: public_keys,
                } = self.select_single_route(select_params).await?;
                (route_id, public_keys.get(crypto_kind).unwrap())
            };

        // Get the compile context (shared state for both optimized and unoptimized paths)
        let ctx = self.get_route_compile_context(
            &safety_route_id,
            sr_pubkey,
            safety_spec,
            optimize,
            opt_private_route_id.is_some(),
        )?;

        #[cfg(feature = "verbose-tracing")]
        let optimize = ctx.optimize;

        // Compile using the appropriate method
        let compiled_route = Self::compile_route_inner(&routing_table, params, ctx).await?;

        // Always cache the compiled route (both optimized and unoptimized)
        // Unoptimized routes are cached to pin the safety route selection,
        // and will be upgraded to optimized on cache hit once validated
        entry.insert(compiled_route.clone());

        // Release the cache entry lock (doing this explicitly here for clarity)
        drop(entry);

        // Return compiled route
        #[cfg(feature = "verbose-tracing")]
        veilid_log!(self debug "compile_route profile (uncached{}): {:#}", if optimize { "" } else { " unoptimized" }, Timestamp::now().duration_since(profile_start_ts));

        Ok(compiled_route)
    }

    /// Looks up an existing compiled route from the safety and private route components
    /// Single location for all cache validation logic.
    /// Returns either a compiled route, or a lock to the cache entry and whether or not to compile or optimize
    async fn lookup_compiled_route_cache(
        &self,
        params: &CompileRouteParams,
    ) -> RouteCompileCacheDisposition {
        // Get the compile route cache key for the parameters
        let key: CompiledRouteCacheKey = params.into();

        // Lock this key's cache slot (does not block other keys)
        let entry = self.compiled_route_cache.lookup(key.clone()).await;

        // See if we have anything in the cache for this key
        let Some(compiled_route) = entry.get() else {
            // Nothing in the cache, we have to compile the route, return the locked entry
            return RouteCompileCacheDisposition::Compile(entry);
        };

        // -------------------------------------------------------
        // Cache validation
        //
        // We do all of the cache validation here to avoid having invalidation spread all over the codebase.
        //
        // 1. Safety route in cache key must still exist
        // 2. If the private route being compiled with is allocated it must still exist
        // 3. Check the safety route and private route for validity and optimizability:
        //    a. If the route has a bad stats disposition, it should be invalidated so we have a chance at a different route selection
        //    b. If the route is optimizable, and the cache is optimized it is valid
        //    c. If the route is not optimizable, and the cache is unoptimized it is valid
        //    d. If there is no optimizability match, then we either need to compile, or optimize
        //
        // While allocated routes themselves are only valid while our node info is equivalent to
        // the node info at time of allocation, we do not need to check for this here because
        // if a route is no longer valid, it will be removed from the route spec store cache
        // by the time we get here.
        //
        // The reply private route, if specified is only used to choose a symmetric safety route
        // and is not part of the compile route itself. We do not need to check it explicitly here
        // because checking 'compiled_route.safety_route' already does this for us.

        let cache = self.cache.read();

        // Sequencing this compiled route was built for; an allocated route is invalid here when
        // it is dead for every ordering it provides that matches this sequencing (per-ordering
        // death), rather than the aggregate disposition.
        let sequencing = key.safety_selection.get_sequencing();

        // Check #1: Safety route must still exist
        let safety_route_key = &compiled_route.compiled_safety_route.public_key;
        let Some(safety_route_id) = cache.get_allocated_route_id_by_key(safety_route_key) else {
            #[cfg(feature = "verbose-tracing")]
            veilid_log!(self debug "Safety route no longer exists for key {}, invalidating cache entry {:?}", safety_route_key, key);

            drop(cache);
            entry.remove();
            return RouteCompileCacheDisposition::Compile(entry);
        };

        let safety_route_is_optimizable = {
            // Check #1: Safety route must still exist
            let Some(arce) = cache.get_allocated_route_by_id(&safety_route_id) else {
                #[cfg(feature = "verbose-tracing")]
                veilid_log!(self debug "Safety route no longer exists for id {} with key={}, invalidating cache entry {:?}", safety_route_id, safety_route_key, key);
                drop(cache);
                entry.remove();
                return RouteCompileCacheDisposition::Compile(entry);
            };

            // Check #3a: If the route is dead for this sequencing, invalidate so we get a chance at a different route selection
            if !arce.is_live_sequencing_match(sequencing) {
                #[cfg(feature = "verbose-tracing")]
                veilid_log!(self debug "Safety route dead for sequencing {} for id {} with key={}, invalidating cache entry {:?}", sequencing, safety_route_id, safety_route_key, key);
                drop(cache);
                entry.remove();
                return RouteCompileCacheDisposition::Compile(entry);
            }
            arce.is_route_optimizable()
        };

        // Check #2: Private route must still exist if it is an allocated route
        // XXX: We don't optimize private routes today. Route optimization will go away as part of PR2.0, so for now, we let this be.
        let private_route_key = &key.pr_pubkey;
        if compiled_route.pr_allocated {
            // Allocated private route

            // Check #2: Private route must still exist if it is an allocated route
            let Some(private_route_id) = cache.get_allocated_route_id_by_key(private_route_key)
            else {
                #[cfg(feature = "verbose-tracing")]
                veilid_log!(self debug "Private route no longer exists for key {}, invalidating cache entry {:?}", private_route_key, key);
                drop(cache);
                entry.remove();
                return RouteCompileCacheDisposition::Compile(entry);
            };

            // Check #2: Private route must still exist if it is an allocated route
            let Some(arce) = cache.get_allocated_route_by_id(&private_route_id) else {
                #[cfg(feature = "verbose-tracing")]
                veilid_log!(self debug "Private route no longer exists for id {} with key={}, invalidating cache entry {:?}", private_route_id, private_route_key, key);
                drop(cache);
                entry.remove();
                return RouteCompileCacheDisposition::Compile(entry);
            };

            // Check #3a: If the route is dead for this sequencing, invalidate so we get a chance at a different route selection
            if !arce.is_live_sequencing_match(sequencing) {
                #[cfg(feature = "verbose-tracing")]
                veilid_log!(self debug "Private route dead for sequencing {} for id {} with key={}, invalidating cache entry {:?}", sequencing, private_route_id, private_route_key, key);
                drop(cache);
                entry.remove();
                return RouteCompileCacheDisposition::Compile(entry);
            }
        } else {
            // Non-allocated, but imported, remote private route
            if let Some(private_route_id) = cache.get_remote_route_id_by_key(private_route_key) {
                let cur_ts = Timestamp::now();

                // Check #2: Private route must still exist if it is is imported
                let Some(rrce) = cache.peek_remote_route(cur_ts, &private_route_id) else {
                    #[cfg(feature = "verbose-tracing")]
                    veilid_log!(self debug "Private route no longer exists for id {} with key={}, invalidating cache entry {:?}", private_route_id, private_route_key, key);
                    drop(cache);
                    entry.remove();
                    return RouteCompileCacheDisposition::Compile(entry);
                };

                // Check #3a: Remote route orderings aren't tracked locally, so fall back to the
                // aggregate disposition here (conservative: only invalidate when all orderings dead).
                let disposition = rrce.with_stats(|stats| stats.aggregate_disposition());
                match disposition {
                    RouteStatsDisposition::Valid => {}
                    _ => {
                        #[cfg(feature = "verbose-tracing")]
                        veilid_log!(self debug "Private route was {:?} for id {} with key={}, invalidating cache entry {:?}", disposition, private_route_id, private_route_key, key);
                        drop(cache);
                        entry.remove();
                        return RouteCompileCacheDisposition::Compile(entry);
                    }
                }
            }
        }

        drop(cache);

        // Check a cached compiled route and determine if it can be used as-is or needs recompilation.
        // If the cached route is unoptimized but the safety route and/or local private route has since been validated,
        // removes the stale cache entry and signals recompilation.
        if compiled_route.optimized == safety_route_is_optimizable {
            // If the route has the same optimizability as the cache, then return the cached route
            return RouteCompileCacheDisposition::Cached(compiled_route);
        }

        // Invalidate the cache entry so we can recompile or optimize
        entry.remove();

        // If what was in the cache was optimized, then we need to recompile unoptimized
        // If what was in the cache was unoptimized, then we can optimize it now
        if compiled_route.optimized {
            #[cfg(feature = "verbose-tracing")]
            veilid_log!(self debug "Compiled route needs recompilation for id {} with key={}, invalidating cache entry {:?}", safety_route_id, safety_route_key, key);

            RouteCompileCacheDisposition::Compile(entry)
        } else {
            #[cfg(feature = "verbose-tracing")]
            veilid_log!(self debug "Compiled route needs optimization for id {} with key={}, invalidating cache entry {:?}", safety_route_id, safety_route_key, key);

            RouteCompileCacheDisposition::Optimize(entry)
        }
    }
}
