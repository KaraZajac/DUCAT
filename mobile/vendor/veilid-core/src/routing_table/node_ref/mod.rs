mod filtered_node_ref;
mod node_ref_filter;
mod traits;

impl_veilid_log_facility!("rtab");

use super::*;

pub(crate) use filtered_node_ref::*;
pub(crate) use node_ref_filter::*;
pub(crate) use traits::*;

///////////////////////////////////////////////////////////////////////////
// Default NodeRef

pub(crate) struct NodeRef {
    registry: VeilidComponentRegistry,
    entry: Arc<BucketEntry>,
    #[cfg(feature = "tracking")]
    track_id: usize,
}

impl_veilid_component_accessors!(NodeRef);

impl NodeRef {
    pub fn new(registry: VeilidComponentRegistry, entry: Arc<BucketEntry>) -> Self {
        entry.ref_count.fetch_add(1u32, Ordering::AcqRel);

        #[cfg(feature = "tracking")]
        let track_id = entry.track();

        Self {
            registry,
            entry,
            #[cfg(feature = "tracking")]
            track_id,
        }
    }

    pub fn sequencing_filtered(&self, sequencing: Sequencing) -> FilteredNodeRef {
        FilteredNodeRef::new(
            self.registry.clone(),
            self.entry.clone(),
            NodeRefFilter::new(),
            sequencing,
        )
    }

    pub fn routing_domain_filtered<R: Into<RoutingDomainSet>>(
        &self,
        routing_domain_set: R,
    ) -> FilteredNodeRef {
        FilteredNodeRef::new(
            self.registry.clone(),
            self.entry.clone(),
            NodeRefFilter::new().with_routing_domain_set(routing_domain_set.into()),
            Sequencing::PreferUnordered,
        )
    }

    pub fn custom_filtered(&self, filter: NodeRefFilter) -> FilteredNodeRef {
        FilteredNodeRef::new(
            self.registry.clone(),
            self.entry.clone(),
            filter,
            Sequencing::PreferUnordered,
        )
    }
}

impl NodeRefAccessorsTrait for NodeRef {
    fn entry(&self) -> Arc<BucketEntry> {
        self.entry.clone()
    }

    fn sequencing(&self) -> Sequencing {
        Sequencing::PreferUnordered
    }

    fn routing_domain_set(&self) -> RoutingDomainSet {
        RoutingDomainSet::all()
    }

    fn filter(&self) -> NodeRefFilter {
        NodeRefFilter::new()
    }

    fn take_filter(&mut self) -> NodeRefFilter {
        NodeRefFilter::new()
    }

    fn dial_info_filter(&self) -> DialInfoFilter {
        DialInfoFilter::all()
    }
}

impl NodeRefOperateTrait for NodeRef {
    fn operate<T, F>(&self, f: F) -> T
    where
        F: FnOnce(&BucketEntryInner) -> T,
    {
        self.entry.with(f)
    }

    fn operate_mut<T, F>(&self, f: F) -> T
    where
        F: FnOnce(&mut BucketEntryInner) -> T,
    {
        self.entry.with_mut(f)
    }
}

impl NodeRefCommonTrait for NodeRef {}

impl Clone for NodeRef {
    fn clone(&self) -> Self {
        self.entry.ref_count.fetch_add(1u32, Ordering::AcqRel);

        Self {
            registry: self.registry.clone(),
            entry: self.entry.clone(),
            #[cfg(feature = "tracking")]
            track_id: self.entry.track(),
        }
    }
}

impl fmt::Display for NodeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let best_node_id = self.entry.with(|e| e.best_node_id());
        write!(f, "{}", f.to_string(best_node_id))
    }
}

impl fmt::Debug for NodeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeRef")
            .field("node_ids", &self.entry.with(|e| e.node_ids()))
            .finish()
    }
}

impl Drop for NodeRef {
    fn drop(&mut self) {
        #[cfg(feature = "tracking")]
        self.entry.untrack(self.track_id);

        // drop the noderef and queue a bucket kick if it was the last one
        let new_ref_count = self.entry.ref_count.fetch_sub(1u32, Ordering::AcqRel) - 1;
        if new_ref_count == 0 {
            // get node ids with inner unlocked because nothing could be referencing this entry now
            // and we don't know when it will get dropped, possibly inside a lock
            let node_ids = self.entry.with(|e| e.node_ids());
            self.routing_table().queue_bucket_kicks(node_ids);
        }
    }
}

impl core::hash::Hash for NodeRef {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.entry).hash(state);
    }
}

impl core::cmp::PartialEq for NodeRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.entry, &other.entry)
    }
}

impl core::cmp::Eq for NodeRef {}
