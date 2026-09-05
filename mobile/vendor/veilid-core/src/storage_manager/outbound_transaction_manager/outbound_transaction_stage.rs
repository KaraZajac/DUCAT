use super::*;

/// The operational stage of a transaction
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(in crate::storage_manager) enum OutboundTransactionStage {
    /// Transaction failed locally, only ROLLBACK operation is possible
    Failed,
    /// ROLLBACK completed, transaction is finished
    Rollback,
    /// BEGIN completed, SET, GET and INSPECT operations are now possible
    Begin,
    /// END completed, SET, GET, and INSPECT are no longer accepted, only COMMIT and ROLLBACK are possible
    End,
    /// COMMIT completed, transaction is finished
    Commit,
}

impl fmt::Display for OutboundTransactionStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                OutboundTransactionStage::Failed => "FAILED",
                OutboundTransactionStage::Rollback => "ROLLBACK",
                OutboundTransactionStage::Begin => "BEGIN",
                OutboundTransactionStage::End => "END",
                OutboundTransactionStage::Commit => "COMMIT",
            }
        )
    }
}
