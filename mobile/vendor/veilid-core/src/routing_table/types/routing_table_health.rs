//! Routing Table Health Metrics

use super::*;

/// Externally visible health metrics for the routing table
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[must_use]
pub struct RoutingTableHealth {
    /// Total number of entries in the routing table
    pub total_entry_count: usize,
    /// Number of nodes per bucket entry state
    pub per_state_entry_count: BTreeMap<BucketEntryState, usize>,
    /// Health per routing domain
    pub routing_domain_health: BTreeMap<RoutingDomain, RoutingDomainHealth>,
}

impl RoutingTableHealth {
    /// Number of live entries in the routing table
    #[must_use]
    pub fn live_entry_count(&self) -> usize {
        let mut live_count = 0;
        for (state, count) in self.per_state_entry_count.iter() {
            if state.is_live() {
                live_count += count;
            }
        }
        live_count
    }

    /// Number of responsive entries in the routing table
    #[must_use]
    pub fn responsive_entry_count(&self) -> usize {
        let mut responsive_count = 0;
        for (state, count) in self.per_state_entry_count.iter() {
            if state.is_responsive() {
                responsive_count += count;
            }
        }
        responsive_count
    }

    /// Number of reliable entries in the routing table
    #[must_use]
    pub fn reliable_entry_count(&self) -> usize {
        let mut reliable_count = 0;
        for (state, count) in self.per_state_entry_count.iter() {
            if state.is_reliable() {
                reliable_count += count;
            }
        }
        reliable_count
    }

    /// Whether the given routing domain is ready for use for all operations (inbound and outbound)
    #[must_use]
    pub fn routing_domain_ready(&self, routing_domain: RoutingDomain) -> bool {
        self.routing_domain_health
            .get(&routing_domain)
            .map(|h| h.ready())
            .unwrap_or(false)
    }
}

impl fmt::Display for RoutingTableHealth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut domains_health = Vec::new();

        let mut per_state_counts = Vec::new();
        for (state, count) in self.per_state_entry_count.iter() {
            per_state_counts.push(format!("{}={}", f.to_string(state), count));
        }

        domains_health.push(format!(
            "All Domains:\n    total={}, live={} ({})",
            self.total_entry_count,
            self.live_entry_count(),
            per_state_counts.join(", ")
        ));

        for (rd, h) in self.routing_domain_health.iter() {
            domains_health.push(format!(
                "{}:\n{}",
                f.to_string(rd),
                indent_all_string(f.to_string(h))
            ));
        }
        write!(f, "{}", domains_health.join("\n"))
    }
}
