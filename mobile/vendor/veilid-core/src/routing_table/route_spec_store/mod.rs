use super::*;
use crate::veilid_api::*;

mod allocated_route_cache_entry;
mod remote_route_cache_entry;
mod route_allocate;
mod route_assemble;
mod route_compile;
mod route_ref;
mod route_remote;
mod route_select;
mod route_set_id;
mod route_set_ref;
mod route_set_spec_detail;
mod route_spec_store_cache;
mod route_spec_store_content;
mod route_stats;
mod route_test;
mod route_validate;

use allocated_route_cache_entry::*;
use remote_route_cache_entry::*;
use route_compile::*;
use route_set_spec_detail::*;
use route_spec_store_cache::*;
use route_spec_store_content::*;

pub(crate) use route_allocate::AllocateRouteParams;
pub(crate) use route_compile::{CompileRouteParams, CompiledRoute};
pub(crate) use route_ref::{AllocatedRouteRef, RemoteRouteRef};
pub(crate) use route_select::{RouteIdAndKeys, RouteSelectParams};
pub(crate) use route_set_id::{AllocatedRouteSetId, RemoteRouteSetId};
pub(crate) use route_set_ref::{AllocatedRouteSetRef, RemoteRouteSetRef};
pub use route_stats::*;
pub(crate) use route_test::{RoutePingValidationPurpose, RoutePingValidationRequest};

impl_veilid_log_facility!("rtab");

/// The size of the remote private route cache
const REMOTE_ROUTE_CACHE_SIZE: usize = 1024;
/// Remote private route cache entries expire in 5 minutes if they haven't been used
const REMOTE_PRIVATE_ROUTE_CACHE_EXPIRY: TimestampDuration = TimestampDuration::new(300_000_000u64);
/// Amount of time a route can remain idle before it gets tested
const ROUTE_MIN_IDLE_TIME_MS: u32 = 30_000;
/// Number of unique failing destinations before invalidating compiled route cache for a route
const ROUTE_LOST_DESTINATIONS_INVALIDATE_THRESHOLD: usize = 5;
/// Number of consecutive send failures before invalidating compiled route cache for a safety route
const ROUTE_SEND_FAILURES_INVALIDATE_THRESHOLD: usize = 1;
/// The size of the compiled route cache
const COMPILED_ROUTE_CACHE_SIZE: usize = 1024;

/// Per-key-locked cache of compiled routes (one compile per route at a time)
pub(crate) type CompiledRouteCache = AsyncKeyedCache<CompiledRouteCacheKey, Arc<CompiledRoute>>;
/// A locked entry in the [CompiledRouteCache]
pub(crate) type CompiledRouteCacheEntry =
    AsyncKeyedCacheEntry<CompiledRouteCacheKey, Arc<CompiledRoute>>;

/// The routing table's storage for private/safety routes
/// Lock order is always content before cache
#[derive(Debug)]
#[must_use]
pub(crate) struct RouteSpecStore {
    registry: VeilidComponentRegistry,
    /// Allocated route content
    content: RwLock<RouteSpecStoreContent>,
    /// Allocated route cache, as well as imported remote routes (not persisted)
    cache: RwLock<RouteSpecStoreCache>,
    /// Per-key-locked compiled route cache (one compile per route at a time)
    compiled_route_cache: CompiledRouteCache,
    /// Ensure we don't try to select the first available route for more the same parameters than once at a time
    allocate_route_lock: AsyncMutex<()>,
    /// Maximum number of hops in a route
    max_route_hop_count: usize,
    /// Default number of hops in a safe route
    default_route_hop_count_safe: usize,
    /// Default number of hops in an unsafe route
    default_route_hop_count_unsafe: usize,
    /// Round-robin counter for distributing route selection across available routes
    route_selection_counter: AtomicUsize,
    /// Round-robin counter for distributing route-test destination selection
    test_destination_rotation_counter: AtomicUsize,
}

impl_veilid_component_accessors!(RouteSpecStore);

impl RouteSpecStore {
    pub fn new(registry: VeilidComponentRegistry) -> Self {
        let config = registry.config();

        let max_route_hop_count = config.internal().network.rpc.max_route_hop_count as usize;
        let default_route_hop_count_safe = config.network.rpc.default_route_hop_count as usize;
        let default_route_hop_count_unsafe =
            max_route_hop_count.min(default_route_hop_count_safe * 2);

        Self {
            registry: registry.clone(),
            content: RwLock::new(RouteSpecStoreContent::new(registry.clone())),
            cache: RwLock::new(RouteSpecStoreCache::new(registry.clone())),
            compiled_route_cache: AsyncKeyedCache::new(COMPILED_ROUTE_CACHE_SIZE),
            allocate_route_lock: AsyncMutex::new(()),
            max_route_hop_count,
            default_route_hop_count_safe,
            default_route_hop_count_unsafe,
            route_selection_counter: AtomicUsize::new(0),
            test_destination_rotation_counter: AtomicUsize::new(0),
        }
    }

    pub fn get_max_route_hop_count(&self) -> usize {
        self.max_route_hop_count
    }

    pub fn get_default_route_hop_count_safe(&self) -> usize {
        self.default_route_hop_count_safe
    }

    pub fn get_default_route_hop_count_unsafe(&self) -> usize {
        self.default_route_hop_count_unsafe
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub fn reset(&self) {
        self.content.write().reset();
        *self.cache.write() = RouteSpecStoreCache::new(self.registry());
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip_all, err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn load(&self) -> EyreResult<()> {
        // Get frozen blob from table store
        let mut content = RouteSpecStoreContent::load(self.registry()).await?;
        let mut cache = RouteSpecStoreCache::new(self.registry());

        // Rebuild the routespecstore cache from the persisted content
        let routing_table = self.routing_table();
        let mut dead_specs = Vec::new();
        for (rsid, rssd) in content.iter_details() {
            // Look up node refs
            let Some(hop_node_refs) = rssd.lookup_node_refs(&routing_table) else {
                dead_specs.push(rsid.clone());
                continue;
            };
            if let Err(e) = cache.add_allocated_route(rsid.clone(), rssd, hop_node_refs) {
                veilid_log!(self error "Error adding allocated route: {}", e);
                dead_specs.push(rsid.clone());
            }
        }

        // Drop the dead route specs that we could not longer resolve node refs for
        for rsid in dead_specs {
            content.remove_detail(&rsid);
        }

        // Return the loaded RouteSpecStore
        *self.content.write() = content;
        *self.cache.write() = cache;

        Ok(())
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn save(&self) -> EyreResult<()> {
        // Snapshot our content so save it since this is an async operation
        let content = self.content.read().clone();

        // Save our content to the tablestore
        content.save().await
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip(self), fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub fn send_route_update(&self) {
        let (dead_routes, dead_remote_routes) = {
            let mut cache = self.cache.write();
            let Some(dr) = cache.take_dead_routes() else {
                // Nothing to do
                return;
            };
            dr
        };

        let update = VeilidUpdate::RouteChange(Box::new(VeilidRouteChange {
            dead_routes: dead_routes.into_iter().map(Into::into).collect(),
            dead_remote_routes: dead_remote_routes.into_iter().map(Into::into).collect(),
        }));

        let update_callback = self.registry.update_callback();
        update_callback(update);
    }

    /// Purge the route spec store
    pub async fn purge(&self) -> VeilidAPIResult<()> {
        // Careful with locking order here, we need to lock the content before the cache
        {
            if !self.compiled_route_cache.purge() {
                apibail_internal!(
                    "Failed to purge route spec store cache while it is in use: {:#?}",
                    self.compiled_route_cache
                );
            }

            let mut content = self.content.write();
            let mut cache = self.cache.write();

            content.reset();
            *cache = RouteSpecStoreCache::new(self.registry());
        }

        self.save().await.map_err(VeilidAPIError::internal)
    }

    /// Release an allocated or remote route that is no longer in use
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip(self), ret, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub fn release_route(&self, id: RouteId) -> bool {
        let is_remote = self.is_route_id_remote(&id);
        if is_remote {
            self.release_remote_route_id(RemoteRouteSetId::from_route_id(id))
        } else {
            self.release_allocated_route(AllocatedRouteSetId::from_route_id(id))
        }
    }

    /// List all allocated routes with a filter. Filter must not access the route spec store due to locking.
    pub fn list_allocated_routes<F, R>(&self, mut filter: F) -> Vec<R>
    where
        F: FnMut(&AllocatedRouteSetId, &AllocatedRouteCacheEntry) -> Option<R>,
    {
        let cache = self.cache.read();
        let mut out = Vec::with_capacity(cache.get_allocated_route_count());
        let mut entries = cache.iter_allocated_routes().collect::<Vec<_>>();
        entries.sort_unstable_by(|a, b| {
            let cmp_hop_count = a.1.hop_count().cmp(&b.1.hop_count());
            if cmp_hop_count != cmp::Ordering::Equal {
                return cmp_hop_count;
            }
            let avg_a = a.1.with_stats(|sa| sa.latency.average);
            let avg_b = b.1.with_stats(|sb| sb.latency.average);
            let cmp_avg_latency = avg_a.cmp(&avg_b);
            if cmp_avg_latency != cmp::Ordering::Equal {
                return cmp_avg_latency;
            }
            a.0.cmp(b.0)
        });

        for detail in entries {
            if let Some(x) = filter(detail.0, detail.1) {
                out.push(x);
            }
        }
        out
    }

    /// List all remote routes with a filter. Filter must not access the route spec store due to locking.
    pub fn list_remote_routes<F, R>(&self, mut filter: F) -> Vec<R>
    where
        F: FnMut(&RemoteRouteSetId, &RemoteRouteCacheEntry) -> Option<R>,
    {
        let cache = self.cache.read();
        let cur_ts = Timestamp::now();
        let remote_route_ids = cache.get_remote_route_ids(cur_ts);

        let remote_routes = remote_route_ids
            .iter()
            .filter_map(|id| cache.peek_remote_route(cur_ts, id).map(|x| (id, x)))
            .collect::<Vec<_>>();
        let mut out = Vec::with_capacity(remote_routes.len());

        for (id, rpri) in remote_routes {
            if let Some(x) = filter(id, &rpri) {
                out.push(x);
            }
        }
        out
    }

    /// Invalidate caches when our local peer info changes
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip(self), fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub fn report_peer_info_changed(&self) {
        veilid_log!(self debug "route cache invalidated due to local node info change");

        // Careful with locking order here, we need to lock the content before the cache
        let cache = self.cache.read();

        // Reset cache entries and require republication of routes
        cache.report_peer_info_changed();
    }

    /// Mark route as published
    /// When first deserialized, routes must be re-published in order to ensure they remain
    /// in the RouteSpecStore.
    pub fn mark_route_published(
        &self,
        id: &AllocatedRouteSetId,
        published: bool,
    ) -> VeilidAPIResult<()> {
        let cache = self.cache.read();
        let Some(rssd) = cache.get_allocated_route_by_id(id) else {
            apibail_invalid_target!("route does not exist");
        };
        rssd.set_published(published);
        Ok(())
    }

    /// Convert private route list to binary blob
    pub fn private_routes_to_blob(private_routes: &[Arc<PrivateRoute>]) -> VeilidAPIResult<Bytes> {
        let mut buffer = BytesMut::new();

        // Serialize count
        let pr_count = private_routes.len();
        if pr_count > MAX_CRYPTO_KINDS {
            apibail_internal!("too many crypto kinds to encode blob");
        }
        let pr_count = pr_count as u8;
        buffer.extend_from_slice(&[pr_count]);

        // Serialize stream of private routes
        for private_route in private_routes {
            let mut pr_message = ::capnp::message::Builder::new_default();
            let mut pr_builder = pr_message.init_root::<veilid_capnp::private_route::Builder>();

            encode_private_route(private_route, &mut pr_builder)
                .map_err(VeilidAPIError::internal)?;

            buffer = canonical_message_builder_to_bytes_writer_packed(pr_message, |size| {
                buffer.reserve(size);
                BytesWriter::new_append(buffer)
            })
            .map_err(VeilidAPIError::internal)?
            .into_inner();
        }
        Ok(buffer.freeze())
    }

    /// Display debugging for routes by their public key
    pub fn display_route_by_key(&self, key: &PublicKey) -> String {
        let cache = self.cache.read();

        // Check for allocated route
        if let Some(id) = cache.get_allocated_route_id_by_key(key) {
            let rstr = if let Some(arce) = cache.get_allocated_route_by_id(&id) {
                format!("allocated: {}", arce)
            } else {
                "(route missing)".to_string()
            };
            return format!("{{key={}, id={}, {}}}", key, id, rstr);
        }

        // Check for remote route
        if let Some(rrid) = cache.get_remote_route_id_by_key(key) {
            let cur_ts = Timestamp::now();
            let rstr = if let Some(rpri) = cache.peek_remote_route(cur_ts, &rrid) {
                format!("remote: {}", rpri)
            } else {
                "(route missing)".to_string()
            };
            return format!("{{key={}, id={}, {}}}", key, rrid, rstr);
        }

        format!("{{key={}, id=(missing)", key)
    }

    /// Display debugging for routes by their route id
    #[cfg_attr(not(feature = "debug-api"), expect(dead_code))]
    pub fn display_route_by_id(&self, id: &RouteId) -> String {
        let cache = self.cache.read();

        let cur_ts = Timestamp::now();

        let arsid = AllocatedRouteSetId::from_route_id(id.clone());
        let rrsid = RemoteRouteSetId::from_route_id(id.clone());

        let rstr = if let Some(arce) = cache.get_allocated_route_by_id(&arsid) {
            format!("allocated: {:#}", arce)
        } else if let Some(rrce) = cache.peek_remote_route(cur_ts, &rrsid) {
            format!("remote: {:#}", rrce)
        } else {
            "(route missing)".to_string()
        };
        format!("{{id={}, {}}}", id, rstr)
    }

    /// Debug debugging for routes by their public key
    #[cfg_attr(not(feature = "debug-api"), expect(dead_code))]
    pub fn debug_route_by_key(&self, key: &PublicKey) -> String {
        // Careful with locking order here, we need to lock the content before the cache
        let content = self.content.read();
        let cache = self.cache.read();

        // Check for allocated route
        if let Some(id) = content.get_id_by_key(key) {
            let rstr = if let Some(rssd) = content.get_detail(&id) {
                format!("allocated: {:#?}", rssd)
            } else {
                "(route missing)".to_string()
            };
            return format!(
                "{{\n    key={:?},\n    id={:?},\n    {}\n}}",
                key,
                id,
                indent_all_string(&rstr)
            );
        }

        // Check for remote route
        if let Some(rrid) = cache.get_remote_route_id_by_key(key) {
            let cur_ts = Timestamp::now();
            let rstr = if let Some(rpri) = cache.peek_remote_route(cur_ts, &rrid) {
                format!("remote: {:#?}", rpri)
            } else {
                "(route missing)".to_string()
            };
            return format!(
                "{{\n    key={:?},\n    id={:?},\n    {}\n}}",
                key,
                rrid,
                indent_all_string(&rstr)
            );
        }

        format!("{{\n    key={:?},\n    id=(missing)\n}}", key)
    }

    /// Debug debugging for routes by their route id
    #[cfg_attr(not(feature = "debug-api"), expect(dead_code))]
    pub fn debug_route_by_id(&self, id: &RouteId) -> String {
        // Careful with locking order here, we need to lock the content before the cache
        let content = self.content.read();
        let cache = self.cache.read();

        let cur_ts = Timestamp::now();
        let arsid = AllocatedRouteSetId::from_route_id(id.clone());
        let rrsid = RemoteRouteSetId::from_route_id(id.clone());

        let rstr = if let Some(rssd) = content.get_detail(&arsid) {
            format!("allocated: {:#?}", rssd)
        } else if let Some(rpri) = cache.peek_remote_route(cur_ts, &rrsid) {
            format!("remote: {:#?}", rpri)
        } else {
            "(route missing)".to_string()
        };
        format!("{{\n    id={:?},\n    {}\n}}", id, indent_all_string(&rstr))
    }
}
