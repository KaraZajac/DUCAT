use super::*;

impl_veilid_log_facility!("rtab");

pub trait RoutingDomainController:
    component::VeilidComponentRegistryAccessor + core::any::Any + core::fmt::Debug + Send + Sync
{
    fn as_any(&self) -> &dyn core::any::Any;

    /// The routing domain identifier for this routing domain detail
    fn routing_domain(&self) -> RoutingDomain;

    // Routing Domain Detail Accessors
    ///////////////////////////////////////////////////////////////////////////////////////

    /// Read access to the routing domain detail
    fn read_dyn(&self) -> MappedRwLockReadGuard<'_, dyn RoutingDomainDetail>;
    /// Write access to the routing domain detail
    fn write_dyn(&self) -> MappedRwLockWriteGuard<'_, dyn RoutingDomainDetail>;
    /// Editor access to common fields in the routing domain detail
    fn edit_dyn(&self) -> Box<dyn RoutingDomainEditor + '_>;

    /// Start up the routing domain controller
    fn startup(&self) -> PinBoxFuture<'_, EyreResult<()>>;
    /// Shut down the routing domain controller
    fn shutdown(&self) -> PinBoxFuture<'_, ()>;
    /// Run ticker for this domain
    fn tick(&self) -> PinBoxFuture<'_, EyreResult<()>>;
    /// Cancel tasks for this domain
    fn cancel_tasks(&self) -> PinBoxFuture<'_, ()>;

    /// Load persisted state from the routing table TableDB
    fn load<'a>(&'a self, _db: &'a TableDB) -> PinBoxFuture<'a, EyreResult<()>> {
        Box::pin(async move { Ok(()) })
    }
    /// Save persisted state into the supplied routing table transaction
    fn save<'a>(&'a self, _dbx: &'a TableDBTransaction) -> PinBoxFuture<'a, EyreResult<()>> {
        Box::pin(async move { Ok(()) })
    }

    // Routing Domain Controller Operations
    ///////////////////////////////////////////////////////////////////////////////////////

    /// The state of this routing domain
    fn state(&self) -> RoutingDomainState;
    /// The health status of this domain
    fn get_health(&self) -> RoutingDomainHealth;

    /// Publish current peer info to the world
    fn publish_peer_info(&self) -> bool;
    /// Unpublish peer info from the world
    fn unpublish_peer_info(&self);
    /// Get the published peer info for this routing domain
    fn get_published_peer_info(&self) -> Option<Arc<PeerInfo>>;

    /// Get the contact methods that node A can use to reach node B in this routing domain
    /// Sorted by preference order, leveraging recent failure inforation, and the optional dial info sort in the request
    fn get_contact_methods(&self, request: ContactMethodRequest) -> Vec<ContactMethod>;

    /// Get the best/first contact method that node A can use to reach node B in this routing domain
    /// If no contact method exists, returns None
    fn get_best_contact_method(&self, request: ContactMethodRequest) -> Option<ContactMethod> {
        self.get_contact_methods(request).into_iter().next()
    }

    /// Gets the dial info details, in preference order, to attempt a connection between two nodes
    /// The best available ordering mode is selected from the sequencing constraint,
    /// then the dial info filter is applied, and self-dialing is filtered out.
    #[cfg_attr(feature = "instrument", instrument(level = "trace", target = "rtab", skip(self), fields(__VEILID_LOG_KEY = self.log_key()), ret))]
    fn get_dial_info_details_between_nodes(
        &self,
        from_node: &NodeInfo,
        to_node: &dyn HasDialInfoDetailList,
        dial_info_filter: DialInfoFilter,
        sequencing: Sequencing,
    ) -> Vec<DialInfoDetail> {
        // Consider outbound capabilities
        let (ordering, outbound_dial_info_filter) = DialInfoFilter::all()
            .with_address_type_set(from_node.address_types())
            .with_protocol_type_set(from_node.outbound_protocols())
            .apply_sequencing(sequencing);

        // If the filter is dead then we won't be able to connect
        if dial_info_filter.is_dead() {
            return vec![];
        }

        // Get the sort for this ordering mode
        let ordering_sort: Option<Box<DialInfoDetailSort>> =
            DialInfoDetail::get_ordering_sort(ordering);

        // Get node A's dial info socket addresses so we can check for self-dialing
        let node_a_socket_addresses = from_node
            .dial_info_detail_list()
            .iter()
            .map(|did| did.dial_info.socket_address())
            .collect::<BTreeSet<_>>();

        // Get all dial info we could possibly connect to for node B with the selected sequencing
        // and ensure we don't direct-dial one of our own addresses (would loop back)
        let filter = Box::new(|did: &DialInfoDetail| {
            did.matches_filter(&outbound_dial_info_filter)
                && !node_a_socket_addresses.contains(&did.dial_info.socket_address())
        });
        let mut all_reachable_dial_info =
            to_node.filtered_dial_info_details(ordering_sort.as_deref(), &filter);

        // Get the best available ordering mode for the reachable dial info
        let Some(best_sequence_ordering) = all_reachable_dial_info
            .iter()
            .map(|x| x.dial_info.protocol_type().sequence_ordering())
            .reduce(|a, b| a.max(b))
        else {
            return vec![];
        };

        // Retain only the dial info with the best sequence ordering and matching the dial info filter
        all_reachable_dial_info.retain(|x| {
            (matches!(sequencing, Sequencing::PreferUnordered)
                || x.dial_info.protocol_type().sequence_ordering() == best_sequence_ordering)
                && x.matches_filter(&dial_info_filter)
        });

        // Now return the first dial info detail in the list
        all_reachable_dial_info
    }
}

pub struct RoutingDomainControllerGuard<'a> {
    controller: Arc<dyn RoutingDomainController>,
    _phantom: core::marker::PhantomData<&'a ()>,
}

impl<'a> RoutingDomainControllerGuard<'a> {
    pub fn new(controller: Arc<dyn RoutingDomainController>) -> Self {
        Self {
            controller,
            _phantom: Default::default(),
        }
    }
}

impl<'a> core::ops::Deref for RoutingDomainControllerGuard<'a> {
    type Target = dyn RoutingDomainController;

    fn deref(&self) -> &Self::Target {
        self.controller.as_ref()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

pub trait SpecificRoutingDomainController: RoutingDomainController {
    const ROUTING_DOMAIN: RoutingDomain;
    type Detail: RoutingDomainDetail;
    type Editor<'a>: RoutingDomainEditor;

    fn read(&self) -> RwLockReadGuard<'_, Self::Detail>;
    fn write(&self) -> RwLockWriteGuard<'_, Self::Detail>;
    fn edit(&self) -> Self::Editor<'_>;
}

pub struct SpecificRoutingDomainControllerGuard<'a, C: SpecificRoutingDomainController> {
    controller: Arc<C>,
    _phantom: core::marker::PhantomData<&'a ()>,
}

impl<'a, C: SpecificRoutingDomainController> SpecificRoutingDomainControllerGuard<'a, C> {
    pub fn new(controller: Arc<C>) -> Self {
        Self {
            controller,
            _phantom: Default::default(),
        }
    }
}

impl<'a, C: SpecificRoutingDomainController> core::ops::Deref
    for SpecificRoutingDomainControllerGuard<'a, C>
{
    type Target = C;

    fn deref(&self) -> &Self::Target {
        &self.controller
    }
}
