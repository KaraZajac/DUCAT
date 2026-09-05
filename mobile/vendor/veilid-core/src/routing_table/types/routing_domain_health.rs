use super::*;

/// Externally visible health metrics for a routing domain
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[must_use]
pub struct RoutingDomainHealth {
    /// Entry snapshot summary of the routing domain per CryptoKind
    pub entry_summary: Arc<EntrySummary>,
    /// Low water mark of the routing domain per CryptoKind
    pub low_water_mark: Arc<LowWaterMark>,
    /// If this routing domain is ready for inbound use (dialinfo, relays, peer info, etc)
    pub is_ready_inbound: bool,
    /// If this routing domain is ready for outbound use (dialinfo, bootstrapped, safety routes, etc)
    pub is_ready_outbound: bool,
}

impl RoutingDomainHealth {
    /// Whether or not this routing domain is ready for all use (inbound and outbound)
    #[must_use]
    pub fn ready(&self) -> bool {
        self.is_ready_inbound && self.is_ready_outbound
    }
}

impl fmt::Display for RoutingDomainHealth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut ready = Vec::new();
        if self.is_ready_inbound {
            ready.push("in".to_string());
        }
        if self.is_ready_outbound {
            ready.push("out".to_string())
        }
        if ready.is_empty() {
            ready.push("none".to_string());
        }

        if f.alternate() {
            write!(
                f,
                "Ready: {}\nSummary:\n{}\nLow Water Mark:\n{}",
                ready.join("+"),
                indent_all_string(f.to_string(&self.entry_summary)),
                indent_all_string(f.to_string(&self.low_water_mark))
            )
        } else {
            // Merge entry summary and low water mark into a single string
            let mut out = Vec::new();
            for ck in VALID_CRYPTO_KINDS {
                let mut ckout = Vec::new();

                if let Some(esumstr) = self
                    .entry_summary
                    .per_crypto_kind
                    .get(&ck)
                    .map(|x| x.to_string())
                {
                    ckout.push(esumstr);
                }
                if let Some(lwmstr) = self
                    .low_water_mark
                    .per_crypto_kind
                    .get(&ck)
                    .map(|x| x.to_string())
                {
                    ckout.push(format!("LWM:{}", lwmstr));
                }
                out.push(ckout.join(" / "));
            }
            if out.is_empty() {
                out.push("None".to_string());
            }

            write!(f, "Ready: {}, {}", ready.join("+"), out.join(", "))
        }
    }
}
