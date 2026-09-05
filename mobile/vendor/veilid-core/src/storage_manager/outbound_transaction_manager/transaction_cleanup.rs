use super::*;

/// Resources that need cleanup when a transaction is dropped
#[must_use]
pub(in crate::storage_manager) struct TransactionCleanup {
    /// Background tokens to wait on
    background_tokens: Vec<StopToken>,
}

impl TransactionCleanup {
    pub(super) fn new(background_tokens: Vec<StopToken>) -> Self {
        Self { background_tokens }
    }

    pub fn merge(&mut self, other: Self) {
        self.background_tokens.extend(other.background_tokens);
    }
}

impl core::future::IntoFuture for TransactionCleanup {
    type Output = ();
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            if !self.background_tokens.is_empty() {
                let mut unord = FuturesUnordered::from_iter(self.background_tokens);
                while (unord.next().await).is_some() {}
            }
        })
    }
}
