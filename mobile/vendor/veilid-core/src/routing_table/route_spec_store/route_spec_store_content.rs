use super::*;

impl_veilid_log_facility!("rtab::route");

/// Serializable data structure for the route spec store content
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RouteSpecStoreContentData {
    /// All of the route sets we have allocated so far indexed by key (many to one)
    id_by_key: HashMap<PublicKey, AllocatedRouteSetId>,
    /// All of the route sets we have allocated so far
    details: HashMap<AllocatedRouteSetId, RouteSetSpecDetail>,
}

/// The core representation of the RouteSpecStore that can be serialized
#[derive(Debug, Clone)]
pub(super) struct RouteSpecStoreContent {
    /// Registry accessor
    registry: VeilidComponentRegistry,
    /// Persistent data for the route spec store content
    data: RouteSpecStoreContentData,
}

impl_veilid_component_accessors!(RouteSpecStoreContent);

impl RouteSpecStoreContent {
    pub fn new(registry: VeilidComponentRegistry) -> Self {
        Self {
            registry,
            data: Default::default(),
        }
    }

    /// Load the route spec store content from the table store
    pub async fn load(registry: VeilidComponentRegistry) -> EyreResult<Self> {
        let table_store = registry.table_store();

        let mut content = RouteSpecStoreContent::new(registry);

        // Deserialize what we can
        let rsstdb = table_store.open("RouteSpecStore", 1).await?;

        content.data = rsstdb.load_json(0, b"content").await?.unwrap_or_default();

        Ok(content)
    }

    /// Save all the fields we care about to the frozen blob in table storage
    /// This skips #[with(Skip)] saving the secret keys, we save them in the protected store instead
    pub async fn save(&self) -> EyreResult<()> {
        let table_store = self.table_store();

        let rsstdb = table_store.open("RouteSpecStore", 1).await?;
        rsstdb.store_json(0, b"content", &self.data).await?;

        Ok(())
    }

    /// Clear the persistent content in memory, does not clear from table storage yet
    pub fn reset(&mut self) {
        self.data = Default::default();
    }

    /// Add a route set spec detail to the persistent content
    pub fn add_detail(&mut self, id: AllocatedRouteSetId, detail: RouteSetSpecDetail) {
        if self.data.details.contains_key(&id) {
            veilid_log!(self error "route set detail already exists, overwriting: {}", id);
        }

        // also store in id by key table
        for (pk, _) in detail.iter_route_set() {
            self.data.id_by_key.insert(pk.clone(), id.clone());
        }
        self.data.details.insert(id, detail);
    }
    pub fn remove_detail(&mut self, id: &AllocatedRouteSetId) -> Option<RouteSetSpecDetail> {
        let detail = self.data.details.remove(id)?;
        for (pk, _) in detail.iter_route_set() {
            let _ = self.data.id_by_key.remove(pk).unwrap_or_log();
        }
        Some(detail)
    }
    pub fn get_detail(&self, id: &AllocatedRouteSetId) -> Option<&RouteSetSpecDetail> {
        self.data.details.get(id)
    }
    pub fn get_id_by_key(&self, key: &PublicKey) -> Option<AllocatedRouteSetId> {
        self.data.id_by_key.get(key).cloned()
    }
    pub fn iter_details(
        &self,
    ) -> std::collections::hash_map::Iter<'_, AllocatedRouteSetId, RouteSetSpecDetail> {
        self.data.details.iter()
    }

    pub fn update_latency(&self, cache: &RouteSpecStoreCache) {
        for (id, rssd) in self.data.details.iter() {
            if let Some(arce) = cache.get_allocated_route_by_id(id) {
                arce.with_stats(|s| rssd.update_latency(s));
            }
        }
    }

    pub fn update_transfers(&self, cache: &RouteSpecStoreCache) {
        for (id, rssd) in self.data.details.iter() {
            if let Some(arce) = cache.get_allocated_route_by_id(id) {
                arce.with_stats(|s| rssd.update_transfers(s));
            }
        }
    }

    pub fn update_answers(&self, cache: &RouteSpecStoreCache) {
        for (id, rssd) in self.data.details.iter() {
            if let Some(arce) = cache.get_allocated_route_by_id(id) {
                arce.with_stats(|s| rssd.update_answers(s));
            }
        }
    }
}
