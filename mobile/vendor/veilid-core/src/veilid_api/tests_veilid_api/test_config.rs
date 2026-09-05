use super::*;
use crate::tests::*;

// The `internal()` accessor must ignore configured "footgun" tuning unless the
// `footgun-config` feature is enabled. This is the off-build runtime gate: a config
// carrying internal overrides still deserializes, but defaults are used.
pub fn test_internal_footgun_gate() {
    // Build a config whose internal tuning differs from the default.
    let mut tuned = VeilidConfigInternal::default();
    tuned.network.rpc.timeout_ms = tuned.network.rpc.timeout_ms.wrapping_add(1);
    assert_ne!(tuned, VeilidConfigInternal::default());

    let mut cfg = fake_veilid_config();
    cfg.internal = Some(tuned.clone());

    #[cfg(feature = "footgun-config")]
    {
        // With the feature, the configured tuning is honored.
        assert_eq!(*cfg.internal(), tuned);
    }
    #[cfg(not(feature = "footgun-config"))]
    {
        // Without the feature, footgun tuning is ignored and built-in defaults are used.
        assert_eq!(*cfg.internal(), VeilidConfigInternal::default());
    }
}
