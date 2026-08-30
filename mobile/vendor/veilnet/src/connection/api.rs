use std::sync::Arc;

use async_trait::async_trait;
use veilid_core::{
    Crypto, CryptoKind, OperationId, RouteBlob, RouteId, Sequencing, Stability, TableStore,
    VeilidAPIResult, VeilidComponentGuard, VeilidConfig, VeilidState,
};

use crate::connection::RoutingContext;

// API presents an interface similar to veilid_core::VeilidAPI methods, through
// a trait that can be wrapped with middleware or mocked entirely.
#[async_trait]
pub trait API {
    async fn shutdown(self) -> ();
    fn is_shutdown(&self) -> bool;
    fn config(&self) -> VeilidAPIResult<Arc<VeilidConfig>>;
    fn crypto<'a>(&'a self) -> VeilidAPIResult<VeilidComponentGuard<'a, Crypto>>;
    fn table_store<'a>(&'a self) -> VeilidAPIResult<VeilidComponentGuard<'a, TableStore>>;
    async fn get_state(&self) -> VeilidAPIResult<VeilidState>;
    async fn attach(&self) -> VeilidAPIResult<()>;
    async fn detach(&self) -> VeilidAPIResult<()>;
    fn routing_context(&self) -> VeilidAPIResult<impl RoutingContext + Send>;
    async fn new_private_route(&self) -> VeilidAPIResult<RouteBlob>;
    async fn new_custom_private_route(
        &self,
        crypto_kinds: &[CryptoKind],
        stability: Stability,
        sequencing: Sequencing,
    ) -> VeilidAPIResult<RouteBlob>;
    fn import_remote_private_route(&self, blob: Vec<u8>) -> VeilidAPIResult<RouteId>;
    fn release_private_route(&self, route_id: RouteId) -> VeilidAPIResult<()>;
    async fn app_call_reply(&self, call_id: OperationId, message: Vec<u8>) -> VeilidAPIResult<()>;
}
