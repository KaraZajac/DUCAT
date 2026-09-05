use super::*;
impl_veilid_log_facility!("rtab");

////////////////////////////////////////////////////////////////////////////////////////////////////
// Compiled Privacy Objects

/// An encrypted private/safety route hop
#[derive(Clone)]
pub(crate) struct RouteHopData {
    /// The nonce used in the encryption ENC(Xn,DH(PKn,SKapr))
    pub nonce: Nonce,
    /// The encrypted blob
    pub blob: Bytes,
}

impl fmt::Debug for RouteHopData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteHopData")
            .field("nonce", &self.nonce)
            .field("blob", &format!("len={}", self.blob.len()))
            .finish()
    }
}

/// How to find a route node
#[derive(Clone, Debug)]
pub(crate) enum RouteNode {
    /// Route node is optimized, no contact method information as this node id has been seen before
    NodeId(NodeId),
    /// Route node with full contact method information to ensure the peer is reachable
    PeerInfo(Arc<PeerInfo>),
}

impl RouteNode {
    /// Extract the node id without consulting the routing table.
    pub fn node_id(&self) -> Option<NodeId> {
        match self {
            RouteNode::NodeId(id) => Some(id.clone()),
            RouteNode::PeerInfo(pi) => pi.node_ids().first().cloned(),
        }
    }

    pub fn node_ref(&self, routing_table: &RoutingTable) -> Option<NodeRef> {
        match self {
            RouteNode::NodeId(id) => {
                //
                match routing_table.lookup_node_id(id.clone()) {
                    Ok(nr) => nr,
                    Err(e) => {
                        veilid_log!(routing_table debug "failed to look up route node: {}", e);
                        None
                    }
                }
            }
            RouteNode::PeerInfo(pi) => {
                //
                match routing_table.register_node_with_peer_info(pi.clone(), false) {
                    Ok(nr) => Some(nr.unfiltered()),
                    Err(e) => {
                        veilid_log!(routing_table debug "failed to register route node: {}", e);
                        None
                    }
                }
            }
        }
    }

    pub fn describe(&self) -> String {
        match self {
            RouteNode::NodeId(id) => {
                format!("{}", id)
            }
            RouteNode::PeerInfo(pi) => match pi.node_ids().first() {
                Some(id) => format!("{}", id),
                None => {
                    format!("?({})", pi.node_ids())
                }
            },
        }
    }
}

/// An unencrypted private/safety route hop
#[derive(Clone, Debug)]
pub(crate) struct RouteHop {
    /// The location of the hop
    pub node: RouteNode,
    /// The encrypted blob to pass to the next hop as its data (None for stubs)
    pub next_hop: Option<RouteHopData>,
}

/// The kind of hops a private route can have
#[derive(Clone, Debug)]
pub(crate) enum PrivateRouteHops {
    /// The first hop of a private route, unencrypted, route_hops == total hop count
    FirstHop(Box<RouteHop>),
    /// Private route internal node. Has > 0 private route hops left but < total hop count
    Data(RouteHopData),
    /// Private route has ended (hop count = 0)
    Empty,
}

/// A private route for receiver privacy
#[derive(Clone, Debug)]
pub(crate) struct PrivateRoute {
    /// The public key used for the entire route
    pub public_key: PublicKey,
    pub hops: PrivateRouteHops,
}

impl PrivateRoute {
    /// Stub route is the form used when no privacy is required, but you need to specify the destination for a safety route
    pub fn new_stub(public_key: PublicKey, node: RouteNode) -> Self {
        Self {
            public_key,
            hops: PrivateRouteHops::FirstHop(Box::new(RouteHop {
                node,
                next_hop: None,
            })),
        }
    }

    /// Check if this is a stub route
    pub fn is_stub(&self) -> bool {
        if let PrivateRouteHops::FirstHop(first_hop) = &self.hops {
            return first_hop.next_hop.is_none();
        }
        false
    }

    /// Get the crypto kind in use for this route
    pub fn crypto_kind(&self) -> CryptoKind {
        self.public_key.kind()
    }

    /// Remove the first unencrypted hop if possible
    pub fn split_first_hop(&self) -> (Option<RouteNode>, Self) {
        let mut opt_route_node = None;
        let out = Self {
            public_key: self.public_key.clone(),
            hops: match &self.hops {
                PrivateRouteHops::FirstHop(first_hop) => {
                    opt_route_node = Some(first_hop.node.clone());

                    match first_hop.next_hop.as_ref() {
                        Some(rhd) => PrivateRouteHops::Data(rhd.clone()),
                        None => PrivateRouteHops::Empty,
                    }
                }
                PrivateRouteHops::Data(_) | PrivateRouteHops::Empty => self.hops.clone(),
            },
        };

        (opt_route_node, out)
    }

    pub fn first_hop_node_id(&self) -> Option<NodeId> {
        let PrivateRouteHops::FirstHop(pr_first_hop) = &self.hops else {
            return None;
        };

        // Get the safety route to use from the spec
        Some(match &pr_first_hop.node {
            RouteNode::NodeId(n) => n.clone(),
            RouteNode::PeerInfo(p) => p.node_ids().get(self.public_key.kind()).unwrap_or_log(),
        })
    }
}

impl fmt::Display for PrivateRoute {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "PR({:?}+{})",
            self.public_key,
            match &self.hops {
                PrivateRouteHops::FirstHop(_) => {
                    format!(
                        "->{}",
                        self.first_hop_node_id()
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| "None".to_owned())
                    )
                }
                PrivateRouteHops::Data(_) => {
                    "->?".to_owned()
                }
                PrivateRouteHops::Empty => {
                    "".to_owned()
                }
            }
        )
    }
}

#[derive(Clone, Debug)]
pub(crate) enum SafetyRouteHops {
    /// Has >= 1 safety route hops
    Data(RouteHopData),
    /// Has 0 safety route hops
    Private(Arc<PrivateRoute>),
}

#[derive(Clone, Debug)]
pub(crate) struct SafetyRoute {
    pub public_key: PublicKey,
    pub hops: SafetyRouteHops,
}

impl SafetyRoute {
    /// Stub route is the form used when no privacy is required, but you need to directly contact a private route
    pub fn new_stub(public_key: PublicKey, private_route: Arc<PrivateRoute>) -> Self {
        // First hop should have already been popped off for stubbed safety routes since
        // we are sending directly to the first hop
        debug_assert!(matches!(private_route.hops, PrivateRouteHops::Data(_)));
        Self {
            public_key,
            hops: SafetyRouteHops::Private(private_route),
        }
    }

    /// Check if this is a stub route
    pub fn is_stub(&self) -> bool {
        matches!(self.hops, SafetyRouteHops::Private(_))
    }

    /// Get the crypto kind in use for this route
    pub fn crypto_kind(&self) -> CryptoKind {
        self.public_key.kind()
    }
}

impl fmt::Display for SafetyRoute {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "SR({:?}+{})",
            self.public_key,
            match &self.hops {
                SafetyRouteHops::Data(_) => "".to_owned(),
                SafetyRouteHops::Private(p) => format!("->{}", p),
            }
        )
    }
}
