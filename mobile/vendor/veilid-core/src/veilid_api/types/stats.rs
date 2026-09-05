use super::*;

/// Measurement of communications latency to this node over all RPC questions
#[apply(api_data_struct!)]
#[api(eq, default, ts)]
pub struct LatencyStats {
    /// fastest latency in the ROLLING_LATENCIES_SIZE last latencies
    pub fastest: TimestampDuration,
    /// average latency over the ROLLING_LATENCIES_SIZE last latencies
    pub average: TimestampDuration,
    /// slowest latency in the ROLLING_LATENCIES_SIZE last latencies
    pub slowest: TimestampDuration,
    /// trimmed mean with lowest 90% latency in the ROLLING_LATENCIES_SIZE
    #[serde(default)]
    pub tm90: TimestampDuration,
    /// trimmed mean with lowest 75% latency in the ROLLING_LATENCIES_SIZE
    #[serde(default)]
    pub tm75: TimestampDuration,
    /// p90 latency in the ROLLING_LATENCIES_SIZE
    #[serde(default)]
    pub p90: TimestampDuration,
    /// p75 latency in the ROLLING_LATENCIES_SIZE
    #[serde(default)]
    pub p75: TimestampDuration,
}

impl fmt::Display for LatencyStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} slow | {} avg | {} fast | {} tm90 | {} tm75 | {} p90 | {} p75",
            f.to_string(self.slowest),
            f.to_string(self.average),
            f.to_string(self.fastest),
            f.to_string(self.tm90),
            f.to_string(self.tm75),
            f.to_string(self.p90),
            f.to_string(self.p75)
        )?;
        Ok(())
    }
}

/// Measurement of how much data has transferred to or from this node over a time span
#[apply(api_data_struct!)]
#[api(eq, default, ts)]
pub struct TransferStats {
    /// total amount transferred ever
    pub total: ByteCount,
    /// maximum rate over the ROLLING_TRANSFERS_SIZE last amounts
    pub maximum: ByteCount,
    /// average rate over the ROLLING_TRANSFERS_SIZE last amounts
    pub average: ByteCount,
    /// minimum rate over the ROLLING_TRANSFERS_SIZE last amounts
    pub minimum: ByteCount,
}

impl fmt::Display for TransferStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/s min | {}/s avg | {}/s max | {} total",
            f.to_string(self.minimum),
            f.to_string(self.average),
            f.to_string(self.maximum),
            if f.alternate() {
                format!(
                    "{} ({} bytes)",
                    human_byte_count(self.total.as_u64()),
                    self.total.as_u64()
                )
            } else {
                format!("{}", self.total.as_u64())
            },
        )?;
        Ok(())
    }
}

/// Transfer statistics in both directions: from a node to us (down) and from us
/// to the node (up).
#[apply(api_data_struct!)]
#[api(eq, default, ts)]
pub struct TransferStatsDownUp {
    /// Transfer from the node to us.
    pub down: TransferStats,
    /// Transfer from us to the node.
    pub up: TransferStats,
}

impl fmt::Display for TransferStatsDownUp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Down: {}\nUp:   {}",
            f.to_string(&self.down),
            f.to_string(&self.up)
        )
    }
}

/// API-visible statistics for a peer in the routing table
#[apply(api_data_struct!)]
#[api(eq, default, ts)]
pub struct PeerStats {
    #[serde(default)]
    /// latency stats for this peer
    pub latency: Option<LatencyStats>,
    /// transfer stats for this peer
    #[serde(default)]
    pub transfer: TransferStatsDownUp,
}

impl fmt::Display for PeerStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "latency: {}", f.to_string_opt(self.latency.as_ref()))?;
        write!(
            f,
            "transfer:\n{}",
            indent_all_string(f.to_string(&self.transfer))
        )?;

        Ok(())
    }
}
