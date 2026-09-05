use super::*;

/// Frequency to flush record stores to disk
pub const FLUSH_RECORD_STORES_INTERVAL_SECS: u32 = 1;
/// Frequency to save metadata to disk
pub const SAVE_METADATA_INTERVAL_SECS: u32 = 30;
/// Frequency to check for offline subkeys writes to send to the network
pub const OFFLINE_SUBKEY_WRITES_INTERVAL_SECS: u32 = 1;
/// Total number of offline subkey write requests to process in parallel
pub const OFFLINE_SUBKEY_WRITES_BATCH_SIZE: usize = 16;
/// Number of subkeys per offline subkey write keys to process in a chunk
pub const OFFLINE_SUBKEY_WRITES_SUBKEY_CHUNK_SIZE: u64 = 4;
/// Frequency to send ValueChanged notifications to the network
pub const SEND_VALUE_CHANGES_INTERVAL_SECS: u32 = 1;
/// Frequency to check for dead nodes and routes for client-side outbound watches
pub const CHECK_OUTBOUND_WATCHES_INTERVAL_SECS: u32 = 1;
/// Frequency to reconcile watches that are at consensus
pub const RECONCILE_OUTBOUND_WATCHES_INTERVAL: TimestampDuration = TimestampDuration::new_secs(60);
/// Frequency to retry reconciliation of watches that are not at consensus
pub const RECONCILE_OUTBOUND_WATCHES_RETRY_INTERVAL: TimestampDuration =
    TimestampDuration::new_secs(5);
/// How long before expiration to try to renew per-node watches
pub const RENEW_OUTBOUND_WATCHES_DURATION: TimestampDuration = TimestampDuration::new_secs(30);
/// Frequency to check for expired server-side watched records
pub const CHECK_INBOUND_WATCHES_INTERVAL_SECS: u32 = 1;
/// Frequency to check for expired client-side transactions
pub const CHECK_OUTBOUND_TRANSACTIONS_INTERVAL_SECS: u32 = 1;
/// Multiplier applied to rpc_timeout_ms to derive the server-side transaction TTL
pub const TRANSACTION_TIMEOUT_RPC_MULTIPLIER: u64 = 2;
/// Multiplier applied to a node's RTT to derive a post-consensus deadline (capped at `rpc.timeout_ms`)
pub const POST_CONSENSUS_RTT_FACTOR: u64 = 3;
/// Headroom beyond `consensus_count` (= `set_value_count`) that Begin aims for, as a percentage rounded up
pub const CONSENSUS_TRIM_PERCENT: usize = 50;
/// Frequency to check for expired server-side transactions
pub const CHECK_INBOUND_TRANSACTIONS_INTERVAL_SECS: u32 = 1;
/// Frequency to process record rehydration requests
pub const REHYDRATE_RECORDS_INTERVAL_SECS: u32 = 1;
/// Number of rehydration requests to process in parallel
pub const REHYDRATE_BATCH_SIZE: usize = 16;
/// Maximum rehydration requests to start per tick
pub const REHYDRATE_REQUESTS_PER_TICK: usize = 4;
/// Number of subkey sets to push in parallel during transactional rehydration
pub const REHYDRATE_SET_CONCURRENCY: usize = 8;
/// Maximum 'offline lag' before we decide to poll for changed watches
pub const CHANGE_INSPECT_LAG: TimestampDuration = TimestampDuration::new_secs(2);
/// Minimum time between lag-triggered all-watch change inspections
pub const CHANGE_INSPECT_LAG_MIN_INTERVAL: TimestampDuration = TimestampDuration::new_secs(30);
/// Delay before retrying a contended change inspection
pub const CHANGE_INSPECT_RETRY_INTERVAL: TimestampDuration = TimestampDuration::new_secs(5);
/// How long after foreground transaction activity background transactions stay deferred
pub const BACKGROUND_TRANSACTION_QUIET_INTERVAL: TimestampDuration = TimestampDuration::new_secs(5);
/// Length of descriptor cache (512 records and 5 nodes per record, roughly 184320 bytes)
pub const DESCRIPTOR_CACHE_SIZE: usize = 2560;
/// Table store table for storage manager metadata
pub const STORAGE_MANAGER_METADATA: &str = "storage_manager_metadata";
/// Storage manager metadata key name for offline subkey write persistence
pub const OFFLINE_SUBKEY_WRITES: &[u8] = b"offline_subkey_writes";
/// Outbound watch manager metadata key name for watch persistence
pub const OUTBOUND_WATCH_MANAGER: &[u8] = b"outbound_watch_manager";
/// Rehydration requests metadata key name for rehydration persistence
pub const REHYDRATION_REQUESTS: &[u8] = b"rehydration_requests";
/// Descriptor cache metadata key name for persistence
pub const DESCRIPTOR_CACHE: &[u8] = b"descriptor_cache";
/// Remote store connection pool divisor
pub const REMOTE_POOL_CONCURRENCY_DIVISOR: usize = 4;

// --- Outbound Transaction Manager Constants ---

/// Upload budget floor in bytes. Effectively-unlimited for now: bandwidth gating
/// awaits the AIMD controller, so the operation_concurrency count cap is the only
/// active throttle. (raise-only; reset on network change)
pub const GLOBAL_OPERATION_UPLOAD_BANDWIDTH_DEFAULT_BYTES: u64 = u64::MAX;
/// Download budget floor in bytes. Effectively-unlimited (see upload).
pub const GLOBAL_OPERATION_DOWNLOAD_BANDWIDTH_DEFAULT_BYTES: u64 = u64::MAX;
/// Fraction (1/N) of rpc.timeout_ms that in-flight operation bytes should drain within
pub const OPERATION_BANDWIDTH_QUEUE_TIME_FRACTION: u64 = 3;
/// Floor of the keepalive interval as a fraction (1/N) of the server-side TTL.
pub const KEEPALIVE_INTERVAL_FLOOR_TTL_FRACTION: u64 = 10;
/// Ceiling of the keepalive interval as a fraction (1/N) of the server-side TTL.
pub const KEEPALIVE_INTERVAL_CEILING_TTL_FRACTION: u64 = 3;
/// Number of RTTs of safety the next reply should land before server expiration.
pub const KEEPALIVE_SAFETY_RTT_FACTOR: u64 = 3;
