//! O14, first gap: can a client scan an *unconfirmed* transaction through
//! `monero-wallet`'s public API?
//!
//! O14 says no — `scan_transaction` is private and `Scanner::scan` takes a
//! block. But `ScannableBlock` is re-exported with all three fields public, so
//! the question is whether a caller can wrap a mempool transaction in one.
//!
//! This does not scan a real transaction; it establishes the weaker and more
//! useful fact, that the path is *reachable from outside the crate*. If this
//! compiles, the public API admits the call.

use monero_wallet::interface::ScannableBlock;
use monero_wallet::Scanner;

/// Wrap an unconfirmed transaction in a synthetic block and scan it.
///
/// `output_index_for_first_ringct_output` is `None`: an unconfirmed transaction
/// has no global output index yet, and the crate's own documentation says the
/// field is never verified by its API. For the `fast/1` question — *does this
/// transaction pay me, and how much* — indices are not needed; they matter when
/// spending the output later, which cannot happen before confirmation anyway.
pub fn scan_unconfirmed(
    scanner: &mut Scanner,
    block: monero_wallet::block::Block,
    tx: monero_wallet::transaction::Transaction<monero_wallet::transaction::Pruned>,
) -> Result<usize, String> {
    let synthetic = ScannableBlock {
        block,
        transactions: vec![tx],
        output_index_for_first_ringct_output: None,
    };
    scanner
        .scan(synthetic)
        .map(|t| t.not_additionally_locked().len())
        .map_err(|e| format!("{e:?}"))
}
