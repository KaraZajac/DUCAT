use super::*;

impl_veilid_log_facility!("veilid_api");

///////////////////////////////////////////////////////////////////////////////////////

/// Valid destinations for a message sent over a routing context.
#[apply(api_data_enum!)]
#[api(eq, ord, hash)]
pub enum Target {
    /// Node by its node id
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    NodeId(NodeId),
    /// Remote private route by its id.
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    RouteId(RouteId),
}

pub(crate) struct RoutingContextUnlockedInner {
    /// Safety routing requirements.
    safety_selection: SafetySelection,
}

/// Routing contexts are the way you specify the communication preferences for Veilid.
///
/// By default routing contexts have 'safety routing' enabled which offers sender privacy.
/// privacy. To disable this and send RPC operations straight from the node use [RoutingContext::with_safety()] with a [SafetySelection::Unsafe] parameter.
/// To enable receiver privacy, you should send to a private route RouteId that you have imported, rather than directly to a NodeId.
#[derive(Clone)]
#[must_use]
pub struct RoutingContext {
    /// Veilid API handle.
    api: VeilidAPI,
    unlocked_inner: Arc<RoutingContextUnlockedInner>,
}

impl fmt::Debug for RoutingContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoutingContext")
            .field("ptr", &format!("{:p}", Arc::as_ptr(&self.unlocked_inner)))
            .field("safety_selection", &self.unlocked_inner.safety_selection)
            .finish()
    }
}

impl RoutingContext {
    ////////////////////////////////////////////////////////////////

    pub(super) fn try_new(api: VeilidAPI) -> VeilidAPIResult<Self> {
        let config = api.config()?;

        Ok(Self {
            api,
            unlocked_inner: Arc::new(RoutingContextUnlockedInner {
                safety_selection: SafetySelection::Safe(SafetySpec {
                    preferred_route: None,
                    hop_count: config.network.rpc.default_route_hop_count as usize,
                    stability: Stability::Reliable,
                    sequencing: Sequencing::PreferOrdered,
                }),
            }),
        })
    }

    #[must_use]
    pub(crate) fn log_key(&self) -> &str {
        self.api.log_key()
    }

    /// Turn on sender privacy, enabling the use of safety routes. This is the default and
    /// calling this function is only necessary if you have previously disable safety or used other parameters.
    ///
    /// Default values for hop count, stability and sequencing preferences are used.
    ///
    /// * Hop count default is dependent on config, but is set to 1 extra hop.
    /// * Stability default is to choose reliable routes, preferring them over low latency.
    /// * Sequencing default is to prefer ordered before unordered message delivery.
    ///
    /// To customize the safety selection in use, use [RoutingContext::with_safety()].
    ///
    /// Errors with `VeilidAPIError::NotInitialized` if the node is shut down (config unavailable).
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key()), ret))]
    pub fn with_default_safety(self) -> VeilidAPIResult<Self> {
        let this = self.clone();
        record_duration(|| {
            veilid_log!(self debug
            "RoutingContext::with_default_safety(self: {:?})", self);

            let config = self.api.config()?;

            self.with_safety(SafetySelection::Safe(SafetySpec {
                preferred_route: None,
                hop_count: config.network.rpc.default_route_hop_count as usize,
                stability: Stability::Reliable,
                sequencing: Sequencing::PreferOrdered,
            }))
        })
        .inspect_err(log_veilid_api_error!(this))
    }

    /// Use a custom [SafetySelection]. Can be used to disable safety via [SafetySelection::Unsafe].
    ///
    /// Errors with `VeilidAPIError::Generic` if [SafetySelection::Unsafe] is requested without the
    /// `footgun-nodeid-target` feature, or if `hop_count` exceeds the configured max route hop count.
    /// Errors with `VeilidAPIError::InvalidArgument` if a `preferred_route` is set that is not a known route id.
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key()), ret))]
    pub fn with_safety(self, mut safety_selection: SafetySelection) -> VeilidAPIResult<Self> {
        let this = self.clone();
        record_duration(|| {
            veilid_log!(self debug
            "RoutingContext::with_safety(self: {:?}, safety_selection: {:?})", self, safety_selection);

            if let SafetySelection::Unsafe(_) = &safety_selection {
                #[cfg(not(feature = "footgun-nodeid-target"))]
                {
                    apibail_generic!("Unsafe routing mode is not allowed without the 'footgun-nodeid-target' feature enabled");
                }
            }

            if let SafetySelection::Safe(safe) = &mut safety_selection {
                if let Some(preferred_route) = &safe.preferred_route {
                    self.api
                        .core_context()?
                        .routing_table()
                        .check_route_id(preferred_route)?;
                }
                let config = self.api.config()?;
                let default_route_hop_count = config.network.rpc.default_route_hop_count as usize;
                let max_route_hop_count = config.internal().network.rpc.max_route_hop_count as usize;

                if safe.hop_count == 0 {
                    safe.hop_count = default_route_hop_count;
                } else if safe.hop_count > max_route_hop_count {
                    apibail_generic!("hop count must be less than or equal to configured max route hop count");
                }
            }

            Ok(Self {
                api: self.api.clone(),
                unlocked_inner: Arc::new(RoutingContextUnlockedInner { safety_selection }),
            })
        }).inspect_err(log_veilid_api_error!(this))
    }

    /// Use a specified [Sequencing] preference, with or without privacy.
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key()), ret))]
    pub fn with_sequencing(self, sequencing: Sequencing) -> Self {
        record_duration(|| {
            veilid_log!(self debug
            "RoutingContext::with_sequencing(self: {:?}, sequencing: {:?})", self, sequencing);

            Self {
                api: self.api.clone(),
                unlocked_inner: Arc::new(RoutingContextUnlockedInner {
                    safety_selection: match &self.unlocked_inner.safety_selection {
                        SafetySelection::Unsafe(_) => SafetySelection::Unsafe(sequencing),
                        SafetySelection::Safe(safety_spec) => SafetySelection::Safe(SafetySpec {
                            preferred_route: safety_spec.preferred_route.clone(),
                            hop_count: safety_spec.hop_count,
                            stability: safety_spec.stability,
                            sequencing,
                        }),
                    },
                }),
            }
        })
    }

    /// Get the safety selection in use on this routing context.
    pub fn safety(&self) -> SafetySelection {
        self.unlocked_inner.safety_selection.clone()
    }

    /// Get the sequencing used by this routing context
    pub fn sequencing(&self) -> Sequencing {
        match &self.unlocked_inner.safety_selection {
            SafetySelection::Unsafe(sequencing) => *sequencing,
            SafetySelection::Safe(safety_spec) => safety_spec.sequencing,
        }
    }

    /// Get the [VeilidAPI] object that created this [RoutingContext].
    pub fn api(&self) -> VeilidAPI {
        self.api.clone()
    }

    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key()), ret))]
    async fn get_destination(&self, target: Target) -> VeilidAPIResult<rpc_processor::Destination> {
        async {
            let rpc_processor = self.api.core_context()?.rpc_processor();
            let recorder =
                DurationRecorder::new("RoutingContext::get_destination", |name, start| {
                    veilid_log!(self debug
                    "{}[start={:#}](self: {:?}, target: {:?})", name, start, self, target);
                });
            recorder
                .record_fut(
                    async {
                        let dest = Box::pin(rpc_processor.resolve_target_to_destination(
                            target,
                            self.unlocked_inner.safety_selection.clone(),
                        ))
                        .await?;
                        VeilidAPIResult::Ok(dest)
                    },
                    |name, start, dur, ret| {
                        veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                        ret
                    },
                )
                .await
        }
        .await
        .inspect_err(log_veilid_api_error!(self))
    }

    fn check_target(&self, target: &Target) -> VeilidAPIResult<()> {
        match target {
            Target::NodeId(node_id) => {
                self.api
                    .core_context()?
                    .routing_table()
                    .check_node_id(node_id)?;
            }
            Target::RouteId(route_id) => {
                self.api
                    .core_context()?
                    .routing_table()
                    .check_route_id(route_id)?;
            }
        }
        Ok(())
    }

    ////////////////////////////////////////////////////////////////
    // App-level Messaging

    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", skip(message), fields(duration, __VEILID_LOG_KEY = self.log_key(), message_len = message.len(), ret.len)))]
    async fn internal_app_call(&self, target: Target, message: Bytes) -> VeilidAPIResult<Bytes> {
        async {
            self.check_target(&target)?;
            let rpc_processor = self.api.core_context()?.rpc_processor();

            let message_len = message.len();
            let recorder = DurationRecorder::new("RoutingContext::app_call", |name, start| {
                veilid_log!(self debug
                    "{}[start={:#}](self: {:?}, target: {:?}, message_len: {})", name, start, self, target, message_len);
                veilid_log!(self trace "message: {:?}", message);
            });
            recorder.record_fut(
                async {
                    let dest = self.get_destination(target).await?;
                    let answer = VeilidAPIError::from_network_result(Box::pin(rpc_processor.rpc_call_app_call(dest, message)).await?)?;
                    tracing::Span::current().record("ret.len", answer.answer.len());
                    VeilidAPIResult::Ok(answer.answer)
                },
                |name, start, dur, ret| {
                    veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                    ret
                },
            ).await
        }.await.inspect_err(log_veilid_api_error!(self))
    }

    #[cfg(feature = "footgun-nodeid-target")]
    /// App-level bidirectional call that expects a response to be returned.
    ///
    /// Veilid apps may use this for arbitrary message passing.
    ///
    /// * `target` - can be either a direct node id or a private route.
    /// * `message` - an arbitrary message blob of up to 32768 bytes.
    ///
    /// Returns an answer blob of up to 32768 bytes.
    ///
    /// Blocks on the network awaiting the reply; governed by `network.rpc.timeout_ms`.
    ///
    /// Errors with `VeilidAPIError::InvalidArgument` if `target` is an unsupported or malformed node id or route id.
    /// Errors with `VeilidAPIError::NoConnection` if the target node id or remote private route could not be
    /// resolved or no route could be allocated (retryable), `::Timeout` if the reply deadline elapsed (retryable),
    /// `::TryAgain` if a route is temporarily unavailable (retryable), and `::InvalidTarget` if the target is unreachable.
    pub async fn app_call(&self, target: Target, message: Vec<u8>) -> VeilidAPIResult<Vec<u8>> {
        self.internal_app_call(target, message.into())
            .await
            .map(|x| x.into())
    }

    #[cfg(not(feature = "footgun-nodeid-target"))]
    /// App-level bidirectional call that expects a response to be returned.
    ///
    /// Veilid apps may use this for arbitrary message passing.
    ///
    /// * `target` - a private route id
    /// * `message` - an arbitrary message blob of up to 32768 bytes.
    ///
    /// Returns an answer blob of up to 32768 bytes.
    ///
    /// Blocks on the network awaiting the reply; governed by `network.rpc.timeout_ms`.
    ///
    /// Errors with `VeilidAPIError::InvalidTarget` if `target` is a `NodeId` (only `RouteId` is permitted without
    /// the `footgun-nodeid-target` feature). Otherwise errors with `VeilidAPIError::NoConnection` if the route could
    /// not be resolved or allocated (retryable), `::Timeout` if the reply deadline elapsed (retryable), or `::TryAgain`
    /// if a route is temporarily unavailable (retryable).
    pub async fn app_call(&self, target: Target, message: Vec<u8>) -> VeilidAPIResult<Vec<u8>> {
        match target {
            Target::RouteId(_) => self
                .internal_app_call(target, message.into())
                .await
                .map(|x| x.into()),
            Target::NodeId(_) => Err(VeilidAPIError::invalid_target(
                "Only RouteId targets are allowed without the footgun feature",
            )),
        }
    }

    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", skip(message), fields(duration, __VEILID_LOG_KEY = self.log_key(), message_len = message.len()), ret))]
    async fn internal_app_message(&self, target: Target, message: Bytes) -> VeilidAPIResult<()> {
        async {
            self.check_target(&target)?;
            let rpc_processor = self.api.core_context()?.rpc_processor();

            let message_len = message.len();
            let recorder = DurationRecorder::new("RoutingContext::app_message", |name, start| {
                veilid_log!(self debug
                    "{}[start={:#}](self: {:?}, target: {:?}, message_len: {})", name, start, self, target, message_len);
                veilid_log!(self trace "message: {:?}", message);
            });
            recorder.record_fut(
                async {
                    let dest = self.get_destination(target).await?;
                    VeilidAPIError::from_network_result(Box::pin(rpc_processor.rpc_call_app_message(dest, message)).await?)
                },
                |name, start, dur, ret| {
                    veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                    ret
                },
            ).await
        }.await.inspect_err(log_veilid_api_error!(self))
    }

    #[cfg(feature = "footgun-nodeid-target")]
    /// App-level unidirectional message that does not expect any value to be returned.
    ///
    /// Veilid apps may use this for arbitrary message passing.
    ///
    /// * `target` - can be either a direct node id or a private route.
    /// * `message` - an arbitrary message blob of up to 32768 bytes.
    ///
    /// Sends over the network but does not await a reply; returns once the statement is dispatched.
    ///
    /// Errors with `VeilidAPIError::InvalidArgument` if `target` is an unsupported or malformed node id or route id.
    /// Errors with `VeilidAPIError::NoConnection` if the target node id or remote private route could not be
    /// resolved or no route could be allocated (retryable), `::Timeout` if dispatch timed out (retryable),
    /// `::TryAgain` if a route is temporarily unavailable (retryable), and `::InvalidTarget` if the target is unreachable.
    pub async fn app_message(&self, target: Target, message: Vec<u8>) -> VeilidAPIResult<()> {
        self.internal_app_message(target, message.into()).await
    }

    #[cfg(not(feature = "footgun-nodeid-target"))]
    /// App-level unidirectional message that does not expect any value to be returned.
    ///
    /// Veilid apps may use this for arbitrary message passing.
    ///
    /// * `target` - a private route.
    /// * `message` - an arbitrary message blob of up to 32768 bytes.
    ///
    /// Sends over the network but does not await a reply; returns once the statement is dispatched.
    ///
    /// Errors with `VeilidAPIError::InvalidTarget` if `target` is a `NodeId` (only `RouteId` is permitted without
    /// the `footgun-nodeid-target` feature). Otherwise errors with `VeilidAPIError::NoConnection` if the route could
    /// not be resolved or allocated (retryable), `::Timeout` if dispatch timed out (retryable), or `::TryAgain`
    /// if a route is temporarily unavailable (retryable).
    pub async fn app_message(&self, target: Target, message: Vec<u8>) -> VeilidAPIResult<()> {
        match target {
            Target::RouteId(_) => self.internal_app_message(target, message.into()).await,
            Target::NodeId(_) => Err(VeilidAPIError::invalid_target(
                "Only PrivateRoute targets are allowed without the footgun feature",
            )),
        }
    }

    ///////////////////////////////////
    // DHT Records

    /// Creates a new DHT record
    ///
    /// The record is considered 'open' after the create operation succeeds.
    /// * 'kind' - specify a cryptosystem kind to use
    /// * 'schema' - the schema to use when creating the DHT record
    /// * 'owner' - optionally specify an owner keypair to use. If you leave this as None then a random one will be generated. If specified, the crypto kind of the owner must match that of the `kind` parameter
    ///
    /// Returns the newly allocated DHT record's key if successful.
    /// Note: if you pass in an owner keypair this call is a deterministic! This means that if you try to create a new record for a given owner and schema that already exists it *will* fail.
    ///
    /// Local-only: builds and opens the record in the local store without network fanout.
    /// The returned record is left open; close it with [RoutingContext::close_dht_record] or it leaks the open handle.
    ///
    /// Errors with `VeilidAPIError::Generic` if `kind` is an unsupported crypto kind or `owner` is a malformed keypair.
    /// Errors with `VeilidAPIError::InvalidArgument` if `schema` has an invalid subkey/member/writer count, if `owner`
    /// is the wrong crypto kind for `kind`, or if this node's id would be a schema member. Errors with
    /// `VeilidAPIError::NotInitialized` if the node is shut down.
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key()), ret))]
    pub async fn create_dht_record(
        &self,
        kind: CryptoKind,
        schema: DHTSchema,
        owner: Option<KeyPair>,
    ) -> VeilidAPIResult<DHTRecordDescriptor> {
        async {
            Crypto::validate_crypto_kind(kind)?;
            schema.validate()?;
            if let Some(owner) = &owner {
                self.api.crypto()?.check_keypair(owner)?;
            }
            let storage_manager = self.api.core_context()?.storage_manager();

            let recorder = DurationRecorder::new("RoutingContext::create_dht_record", |name, start| {
                veilid_log!(self debug
                    "{}[start={:#}](self: {:?}, schema: {:?}, owner: {:?}, kind: {:?})", name, start, self, schema, owner, kind);
            });
            recorder.record_fut(
                Box::pin(storage_manager.create_record(
                    kind,
                    schema,
                    owner,
                    self.unlocked_inner.safety_selection.clone(),
                )),
                |name, start, dur, ret| {
                    veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                    ret
                },
            ).await
        }.await.inspect_err(log_veilid_api_error!(self))
    }

    /// Opens a DHT record at a specific key.
    ///
    /// Associates a 'default_writer' secret if one is provided to provide writer capability. The
    /// writer can be overridden if specified here via the set_dht_value writer.
    ///
    /// Records may only be opened or created. If a record is re-opened it will use the new writer and routing context
    /// ignoring the settings of the last time it was opened. This allows one to open a record a second time
    /// without first closing it, which will keep the active 'watches' on the record but change the default writer or
    /// safety selection.
    ///
    /// Returns the DHT record descriptor for the opened record if successful.
    ///
    /// Half of an open/close pair: close it with [RoutingContext::close_dht_record] or the open handle and its watches leak.
    /// Re-opening an already-open record is safe and replaces the writer and safety selection in place, preserving active watches.
    /// Returns from the local store without a network round-trip when the record is already local; otherwise blocks on a network inspect (subkey 0), and returns `TryAgain` if offline.
    ///
    /// Errors with `VeilidAPIError::Generic` if `record_key` is an unsupported kind or malformed, or `default_writer`
    /// is a malformed keypair. Errors with `VeilidAPIError::TryAgain` if the record is not yet local and the node is
    /// offline (retryable), `::KeyNotFound` if the record does not exist on the network, and `::NotInitialized` if the
    /// node is shut down.
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key()), ret))]
    pub async fn open_dht_record(
        &self,
        record_key: RecordKey,
        default_writer: Option<KeyPair>,
    ) -> VeilidAPIResult<DHTRecordDescriptor> {
        async {
            self.api
                .core_context()?
                .storage_manager()
                .check_record_key(&record_key)?;
            if let Some(default_writer) = &default_writer {
                self.api.crypto()?.check_keypair(default_writer)?;
            }
            let storage_manager = self.api.core_context()?.storage_manager();

            let recorder = DurationRecorder::new("RoutingContext::open_dht_record", |name, start| {
                veilid_log!(self debug
                    "{}[start={:#}](self: {:?}, key: {:?}, default_writer: {:?})", name, start, self, record_key, default_writer);
            });
            recorder.record_fut(
                storage_manager.open_record(
                    record_key,
                    default_writer,
                    self.unlocked_inner.safety_selection.clone(),
                ),
                |name, start, dur, ret| {
                    veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                    ret
                },
            ).await
        }.await.inspect_err(log_veilid_api_error!(self))
    }

    /// Closes a DHT record at a specific key that was opened with create_dht_record or open_dht_record.
    ///
    /// Closing a record allows you to re-open it with a different routing context.
    ///
    /// The release half of the open/close pair; cancels the record's watch (in the background) and drops any associated transaction.
    /// Blocks holding the record lock until pending writes are flushed to the local store (awaits a disk flush).
    /// Closing a record that is local but not currently open is a no-op; closing one not in the local store returns `KeyNotFound`.
    ///
    /// Errors with `VeilidAPIError::Generic` if `record_key` is an unsupported kind or malformed, and
    /// `::NotInitialized` if the node is shut down. Neither `KeyNotFound` nor these errors are retryable.
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key()), ret))]
    pub async fn close_dht_record(&self, record_key: RecordKey) -> VeilidAPIResult<()> {
        async {
            self.api
                .core_context()?
                .storage_manager()
                .check_record_key(&record_key)?;
            let storage_manager = self.api.core_context()?.storage_manager();

            let recorder =
                DurationRecorder::new("RoutingContext::close_dht_record", |name, start| {
                    veilid_log!(self debug
                    "{}[start={:#}](self: {:?}, key: {:?})", name, start, self, record_key);
                });
            recorder
                .record_fut(
                    Box::pin(storage_manager.close_record(record_key)),
                    |name, start, dur, ret| {
                        veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                        ret
                    },
                )
                .await
        }
        .await
        .inspect_err(log_veilid_api_error!(self))
    }

    /// Waits for any pending offline subkey writes for a DHT record to be flushed to the network.
    ///
    /// Returns immediately with `Ok(true)` if there are no pending writes.
    /// When a `timeout` is specified, returns `Ok(true)` if all pending writes were flushed, or `Ok(false)` if `timeout` elapsed first.
    /// When no `timeout` is specified, waits indefinitely for writes to be flushed and then returns `Ok(true)`.
    /// If the system shuts down while waiting, returns `Err(VeilidAPIError::NotInitialized)`.
    /// Errors with `VeilidAPIError::Generic` if `record_key` is an unsupported kind or malformed.
    ///
    /// Blocks until pending writes flush, the `timeout` elapses, or shutdown; non-blocking when there are no pending writes.
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key()), ret))]
    pub async fn flush_dht_record(
        &self,
        record_key: RecordKey,
        timeout: Option<Duration>,
    ) -> VeilidAPIResult<bool> {
        async {
            self.api
                .core_context()?
                .storage_manager()
                .check_record_key(&record_key)?;
            let storage_manager = self.api.core_context()?.storage_manager();

            let recorder = DurationRecorder::new("RoutingContext::flush_dht_record", |name, start| {
                veilid_log!(self debug
                    "{}[start={:#}](self: {:?}, key: {:?}, timeout: {:?})", name, start, self, record_key, timeout);
            });
            recorder.record_fut(
                Box::pin(storage_manager.flush_record(record_key, timeout)),
                |name, start, dur, ret| {
                    veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                    ret
                },
            ).await
        }
        .await
        .inspect_err(log_veilid_api_error!(self))
    }

    /// Deletes a DHT record at a specific key.
    ///
    /// If the record is opened, it must be closed before it is deleted.
    /// Deleting a record does not delete it from the network, but will remove the storage of the record
    /// locally, and will prevent its value from being refreshed on the network by this node.
    ///
    /// Local-only: closes the record if still open, then removes it from the local store; no network round-trip.
    ///
    /// Errors with `VeilidAPIError::Generic` if `record_key` is an unsupported kind or malformed, `::KeyNotFound`
    /// if the record is not in the local store, and `::NotInitialized` if the node is shut down. None are retryable.
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key()), ret))]
    pub async fn delete_dht_record(&self, record_key: RecordKey) -> VeilidAPIResult<()> {
        async {
            self.api
                .core_context()?
                .storage_manager()
                .check_record_key(&record_key)?;
            let storage_manager = self.api.core_context()?.storage_manager();

            let recorder =
                DurationRecorder::new("RoutingContext::delete_dht_record", |name, start| {
                    veilid_log!(self debug
                    "{}[start={:#}](self: {:?}, key: {:?})", name, start, self, record_key);
                });
            recorder
                .record_fut(
                    Box::pin(storage_manager.delete_record(record_key)),
                    |name, start, dur, ret| {
                        veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                        ret
                    },
                )
                .await
        }
        .await
        .inspect_err(log_veilid_api_error!(self))
    }

    /// Gets the latest value of a subkey.
    ///
    /// May pull the latest value from the network, but by setting 'force_refresh' you can force a network data refresh. Can only be used on opened records.
    ///
    /// Returns `None` if the value subkey has not yet been set.
    /// Returns `Some(data)` if the value subkey has valid data.
    ///
    /// Non-blocking when a local value exists and `force_refresh` is false; otherwise blocks on a network fanout and returns `TryAgain` if offline.
    ///
    /// Errors with `VeilidAPIError::InvalidArgument` if the record is not open, `::Generic` if `record_key` is an
    /// unsupported kind or malformed, `::TryAgain` if a network refresh is needed and the node is offline (retryable),
    /// `::KeyNotFound` if the record no longer exists, and `::NotInitialized` if shut down.
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key()), ret))]
    pub async fn get_dht_value(
        &self,
        record_key: RecordKey,
        subkey: ValueSubkey,
        force_refresh: bool,
    ) -> VeilidAPIResult<Option<ValueData>> {
        async {
            self.api
                .core_context()?
                .storage_manager()
                .check_record_key(&record_key)?;
            let storage_manager = self.api.core_context()?.storage_manager();

            let recorder = DurationRecorder::new("RoutingContext::get_dht_value", |name, start| {
                veilid_log!(self debug
                    "{}[start={:#}](self: {:?}, key: {:?}, subkey: {:?}, force_refresh: {:?})", name, start, self, record_key, subkey, force_refresh);
            });
            recorder.record_fut(
                Box::pin(storage_manager.get_value(record_key, subkey, force_refresh)),
                |name, start, dur, ret| {
                    veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                    ret
                },
            ).await
        }.await.inspect_err(log_veilid_api_error!(self))
    }

    /// Pushes a changed subkey value to the network.
    /// The DHT record must first by opened via open_dht_record or create_dht_record.
    ///
    /// The writer, if specified, will override the 'default_writer' specified when the record is opened.
    ///
    /// Returns `None` if the value was successfully set.
    /// Returns `Some(data)` if the value set was older than the one available on the network.
    ///
    /// Blocks on a network fanout to push the value; when offline or the fanout fails, queues the write for later flush (if `allow_offline`) and returns `Ok(None)`.
    ///
    /// Errors with `VeilidAPIError::InvalidArgument` if the record is not open, `::Generic` if `record_key` is an
    /// unsupported kind or the record is not writable (no writer) or the value fails schema validation (subkey out of
    /// schema range, `data` larger than the per-subkey limit, or wrong writer for the subkey), `::TryAgain` if the
    /// record is currently in a transaction (retryable), `::KeyNotFound` if the record no longer exists, and
    /// `::NotInitialized` if shut down. A failed network fanout does not error; the write is deferred (returns `Ok(None)`).
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", skip(data), fields(duration, __VEILID_LOG_KEY = self.log_key(), data.len = data.len()), ret))]
    pub async fn set_dht_value(
        &self,
        record_key: RecordKey,
        subkey: ValueSubkey,
        data: Vec<u8>,
        options: Option<SetDHTValueOptions>,
    ) -> VeilidAPIResult<Option<ValueData>> {
        async {
            self.api
                .core_context()?
                .storage_manager()
                .check_record_key(&record_key)?;
            let storage_manager = self.api.core_context()?.storage_manager();

            let data_len = data.len();
            let recorder = DurationRecorder::new("RoutingContext::set_dht_value", |name, start| {
                veilid_log!(self debug
                    "{}[start={:#}](self: {:?}, key: {:?}, subkey: {:?}, data: len={}, options: {:?})", name, start, self, record_key, subkey, data_len, options);
            });
            recorder.record_fut(
                Box::pin(storage_manager.set_value(record_key, subkey, data, options)),
                |name, start, dur, ret| {
                    veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                    ret
                },
            ).await
        }.await.inspect_err(log_veilid_api_error!(self))
    }

    /// Add or update a watch to a DHT value that informs the user via an VeilidUpdate::ValueChange callback when the record has subkeys change.
    /// One remote node will be selected to perform the watch and it will offer an expiration time based on a suggestion, and make an attempt to
    /// continue to report changes via the callback. Nodes that agree to doing watches will be put on our 'ping' list to ensure they are still around
    /// otherwise the watch will be cancelled and will have to be re-watched.  Can only be used on opened records.
    ///
    /// There is only one watch permitted per record. If a change to a watch is desired, the previous one will be overwritten.
    /// * `key` is the record key to watch. it must first be opened for reading or writing.
    /// * `subkeys`:
    ///   - None: specifies watching the entire range of subkeys.
    ///   - Some(range): is the the range of subkeys to watch. The range must not exceed 512 discrete non-overlapping or adjacent subranges. If no range is specified, this is equivalent to watching the entire range of subkeys.
    /// * `expiration`:
    ///   - None: specifies a watch with no expiration
    ///   - Some(timestamp): the desired timestamp of when to automatically terminate the watch, in microseconds. If this value is less than `network.rpc.timeout_ms` milliseconds in the future, this function will return an error immediately.
    /// * `count:
    ///   - None: specifies a watch count of u32::MAX
    ///   - Some(count): is the number of times the watch will be sent, maximum. A zero value here is equivalent to a cancellation.
    ///
    /// Returns Ok(true) if a watch is active for this record.
    /// Returns Ok(false) if the entire watch has been cancelled.
    ///
    /// Re-watching the same record replaces the prior watch's desired parameters in place; only one watch exists per record.
    /// Records the desired watch state and returns without a network round-trip; a background task reconciles it with a remote node.
    ///
    /// Errors with `VeilidAPIError::InvalidArgument` if the record is not open or a non-zero `expiration` is sooner than
    /// `network.rpc.timeout_ms` in the future; `::Generic` if `record_key` is an unsupported kind or malformed, or no
    /// local record is found; and `::NotInitialized` if shut down. None are retryable; no network errors surface here
    /// since reconciliation is deferred to a background task.
    ///
    /// DHT watches are accepted with the following conditions:
    /// * First-come first-served basis for arbitrary unauthenticated readers, up to network.dht.public_watch_limit per record.
    /// * If a member (either the owner or a SMPL schema member) has opened the key for writing (even if no writing is performed) then the watch will be signed and guaranteed network.dht.member_watch_limit per writer.
    ///
    /// Members can be specified via the SMPL schema and do not need to allocate writable subkeys in order to offer a member watch capability.
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key()), ret))]
    pub async fn watch_dht_values(
        &self,
        record_key: RecordKey,
        subkeys: Option<ValueSubkeyRangeSet>,
        expiration: Option<Timestamp>,
        count: Option<u32>,
    ) -> VeilidAPIResult<bool> {
        async {
            self.api
                .core_context()?
                .storage_manager()
                .check_record_key(&record_key)?;
            let storage_manager = self.api.core_context()?.storage_manager();

            let recorder = DurationRecorder::new("RoutingContext::watch_dht_values", |name, start| {
                veilid_log!(self debug
                    "{}[start={:#}](self: {:?}, key: {:?}, subkeys: {:?}, expiration: {:?}, count: {:?})", name, start, self, record_key, subkeys, expiration, count);
            });
            let subkeys = subkeys.unwrap_or_default();
            let expiration = expiration.unwrap_or_default();
            let count = count.unwrap_or(u32::MAX);
            recorder.record_fut(
                Box::pin(storage_manager.watch_values(record_key, subkeys, expiration, count)),
                |name, start, dur, ret| {
                    veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                    ret
                },
            ).await
        }.await.inspect_err(log_veilid_api_error!(self))
    }

    /// Cancels a watch early.
    ///
    /// This is a convenience function that cancels watching all subkeys in a range. The subkeys specified here
    /// are subtracted from the currently-watched subkey range.  Can only be used on opened records.
    /// * `subkeys`:
    ///   - None: specifies watching the entire range of subkeys.
    ///   - Some(range): is the the range of subkeys to watch. The range must not exceed 512 discrete non-overlapping or adjacent subranges. If no range is specified, this is equivalent to watching the entire range of subkeys.
    ///
    /// Only the subkey range is changed, the expiration and count remain the same.
    /// If no subkeys remain, the watch is entirely cancelled and will receive no more updates.
    ///
    /// Returns Ok(true) if a watch is active for this record.
    /// Returns Ok(false) if the entire watch has been cancelled.
    ///
    /// A no-op returning `Ok(false)` when no watch is active for the record.
    /// Records the reduced desired watch state and returns without a network round-trip; a background task sends the cancel.
    ///
    /// Errors with `VeilidAPIError::InvalidArgument` if the record is not open, `::Generic` if `record_key` is an
    /// unsupported kind or malformed or no local record is found, and `::NotInitialized` if shut down. None are retryable.
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key()), ret))]
    pub async fn cancel_dht_watch(
        &self,
        record_key: RecordKey,
        subkeys: Option<ValueSubkeyRangeSet>,
    ) -> VeilidAPIResult<bool> {
        async {
            self.api
                .core_context()?
                .storage_manager()
                .check_record_key(&record_key)?;
            let storage_manager = self.api.core_context()?.storage_manager();

            let recorder = DurationRecorder::new("RoutingContext::cancel_dht_watch", |name, start| {
                veilid_log!(self debug
                    "{}[start={:#}](self: {:?}, key: {:?}, subkeys: {:?})", name, start, self, record_key, subkeys);
            });
            let subkeys = subkeys.unwrap_or_default();
            recorder.record_fut(
                Box::pin(storage_manager.cancel_watch_values(record_key, subkeys)),
                |name, start, dur, ret| {
                    veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                    ret
                },
            ).await
        }.await.inspect_err(log_veilid_api_error!(self))
    }

    /// Inspects a DHT record for subkey state.
    /// This is useful for checking if you should push new subkeys to the network, or retrieve the current state of a record from the network
    /// to see what needs updating locally. Can only be used on opened records.
    ///
    /// * `key` is the record key to inspect. it must first be opened for reading or writing.
    /// * `subkeys`:
    ///   - None: specifies inspecting the entire range of subkeys.
    ///   - Some(range): is the the range of subkeys to inspect. The range must not exceed 512 discrete non-overlapping or adjacent subranges.
    ///     If no range is specified, this is equivalent to watching the entire range of subkeys.
    ///
    /// * `scope` is what kind of range the inspection has:
    ///   - DHTReportScope::Local`
    ///     Results will be only for a locally stored record.
    ///     Useful for seeing what subkeys you have locally and which ones have not been retrieved.
    ///
    ///   - `DHTReportScope::SyncGet`
    ///     Return the local sequence numbers and the network sequence numbers with GetValue fanout parameters.
    ///     Provides an independent view of both the local sequence numbers and the network sequence numbers for nodes that
    ///     would be reached as if the local copy did not exist locally.
    ///     Useful for determining if the current local copy should be updated from the network.
    ///
    ///   - `DHTReportScope::SyncSet`
    ///     Return the local sequence numbers and the network sequence numbers with SetValue fanout parameters.
    ///     Provides an independent view of both the local sequence numbers and the network sequence numbers for nodes that
    ///     would be reached as if the local copy did not exist locally.
    ///     Useful for determining if the unchanged local copy should be pushed to the network.
    ///
    ///   - `DHTReportScope::UpdateGet`
    ///     Return the local sequence numbers and the network sequence numbers with GetValue fanout parameters.
    ///     Provides an view of both the local sequence numbers and the network sequence numbers for nodes that
    ///     would be reached as if a GetValue operation were being performed, including accepting newer values from the network.
    ///     Useful for determining which subkeys would change with a GetValue operation.
    ///
    ///   - `DHTReportScope::UpdateSet`
    ///     Return the local sequence numbers and the network sequence numbers with SetValue fanout parameters.
    ///     Provides an view of both the local sequence numbers and the network sequence numbers for nodes that
    ///     would be reached as if a SetValue operation were being performed, including accepting newer values from the network.
    ///     This simulates a SetValue with the initial sequence number incremented by 1, like a real SetValue would when updating.
    ///     Useful for determine which subkeys would change with an SetValue operation.
    ///
    /// Returns `Ok(DHTRecordReport)` with the subkey ranges that were returned that overlapped the schema, and sequence numbers for each of the subkeys in the range.
    ///
    /// `DHTReportScope::Local` is local-only and non-blocking; the Sync/Update scopes block on a network inspect fanout and return `TryAgain` if offline.
    ///
    /// Errors with `VeilidAPIError::InvalidArgument` if the record is not open, `::Generic` if `record_key` is an
    /// unsupported kind or malformed, `::TryAgain` if a network scope is requested and the node is offline (retryable),
    /// and `::NotInitialized` if shut down.
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key()), ret))]
    pub async fn inspect_dht_record(
        &self,
        record_key: RecordKey,
        subkeys: Option<ValueSubkeyRangeSet>,
        scope: DHTReportScope,
    ) -> VeilidAPIResult<DHTRecordReport> {
        async {
            self.api
                .core_context()?
                .storage_manager()
                .check_record_key(&record_key)?;
            let storage_manager = self.api.core_context()?.storage_manager();

            let recorder = DurationRecorder::new("RoutingContext::inspect_dht_record", |name, start| {
                veilid_log!(self debug
                    "{}[start={:#}](self: {:?}, record_key: {:?}, subkeys: {:?}, scope: {:?})", name, start, self, record_key, subkeys, scope);
            });
            let subkeys = subkeys.unwrap_or_default();
            recorder.record_fut(
                Box::pin(storage_manager.inspect_record(record_key, subkeys, scope)),
                |name, start, dur, ret| {
                    veilid_log!(self debug
                        "{}[start={:#} dur={:#}](ret: {:?})", name, start, dur, ret);
                    ret
                },
            ).await
        }.await.inspect_err(log_veilid_api_error!(self))
    }

    ///////////////////////////////////
    // Block Store

    #[cfg(feature = "unstable-blockstore")]
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key()), ret))]
    pub async fn find_block(&self, _block_id: BlockId) -> VeilidAPIResult<Vec<u8>> {
        apibail_internal!("unimplemented");
    }

    #[cfg(feature = "unstable-blockstore")]
    #[cfg_attr(feature = "instrument", instrument(target = "veilid_api", level = "debug", fields(duration, __VEILID_LOG_KEY = self.log_key()), ret,))]
    pub async fn supply_block(&self, _block_id: BlockId) -> VeilidAPIResult<bool> {
        apibail_internal!("unimplemented");
    }
}
