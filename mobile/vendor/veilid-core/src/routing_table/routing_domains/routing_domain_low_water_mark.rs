use super::*;

impl_veilid_log_facility!("rtab");

/// Minimum number of nodes after initial discovery for a routing domain
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LowWaterMark {
    pub per_crypto_kind: BTreeMap<CryptoKind, CapabilityCounts>,
}

impl LowWaterMark {
    pub fn new() -> Self {
        Self {
            per_crypto_kind: BTreeMap::new(),
        }
    }

    pub fn set(&mut self, crypto_kind: CryptoKind, count: CapabilityCounts) {
        self.per_crypto_kind.insert(crypto_kind, count);
    }

    pub fn merge(&mut self, other: &LowWaterMark) {
        for (crypto_kind, count) in other.per_crypto_kind.iter() {
            self.per_crypto_kind
                .entry(*crypto_kind)
                .and_modify(|x| x.min_assign(count))
                .or_insert(*count);
        }
    }
}

impl fmt::Display for LowWaterMark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            let mut out = Vec::new();
            for (ck, cc) in self.per_crypto_kind.iter() {
                out.push(format!("{:#}: {}", ck, cc));
            }
            if out.is_empty() {
                out.push("None".to_string());
            }
            write!(f, "{}", out.join("\n"))
        } else {
            let mut out = Vec::new();
            for (ck, cc) in self.per_crypto_kind.iter() {
                out.push(format!("{}({})", ck, cc));
            }
            if out.is_empty() {
                out.push("None".to_string());
            }
            write!(f, "{}", out.join(", "))
        }
    }
}
