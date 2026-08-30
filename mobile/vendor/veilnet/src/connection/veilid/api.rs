use std::sync::Arc;

use async_trait::async_trait;
use veilid_core::{
    Crypto, OperationId, RouteId, TableStore, VeilidAPI, VeilidAPIResult, VeilidComponentGuard,
    VeilidConfig, VeilidState,
};

pub struct API(pub(crate) VeilidAPI);

impl From<VeilidAPI> for API {
    fn from(api: VeilidAPI) -> Self {
        API(api)
    }
}

#[async_trait]
impl crate::connection::API for API {
    async fn shutdown(self) {
        self.0.shutdown().await
    }

    fn is_shutdown(&self) -> bool {
        self.0.is_shutdown()
    }

    fn config(&self) -> VeilidAPIResult<Arc<VeilidConfig>> {
        self.0.config()
    }

    fn crypto<'a>(&'a self) -> VeilidAPIResult<VeilidComponentGuard<'a, Crypto>> {
        self.0.crypto()
    }

    fn table_store<'a>(&'a self) -> VeilidAPIResult<VeilidComponentGuard<'a, TableStore>> {
        self.0.table_store()
    }

    async fn get_state(&self) -> VeilidAPIResult<VeilidState> {
        self.0.get_state().await
    }

    async fn attach(&self) -> VeilidAPIResult<()> {
        self.0.attach().await
    }

    async fn detach(&self) -> VeilidAPIResult<()> {
        self.0.detach().await
    }

    fn routing_context(&self) -> VeilidAPIResult<impl crate::connection::RoutingContext> {
        Ok(super::routing_context::RoutingContext(
            self.0.routing_context()?,
        ))
    }

    async fn new_private_route(&self) -> veilid_core::VeilidAPIResult<veilid_core::RouteBlob> {
        self.0.new_private_route().await
    }

    async fn new_custom_private_route(
        &self,
        crypto_kinds: &[veilid_core::CryptoKind],
        stability: veilid_core::Stability,
        sequencing: veilid_core::Sequencing,
    ) -> veilid_core::VeilidAPIResult<veilid_core::RouteBlob> {
        self.0
            .new_custom_private_route(veilid_core::PrivateSpec {
                crypto_kinds: crypto_kinds.to_vec(),
                hop_count: 0,
                stability,
                sequencing,
            })
            .await
    }

    fn import_remote_private_route(&self, blob: Vec<u8>) -> VeilidAPIResult<RouteId> {
        self.0.import_remote_private_route(blob)
    }

    fn release_private_route(&self, route_id: RouteId) -> VeilidAPIResult<()> {
        self.0.release_private_route(route_id)
    }

    async fn app_call_reply(&self, call_id: OperationId, message: Vec<u8>) -> VeilidAPIResult<()> {
        self.0.app_call_reply(call_id, message).await
    }
}
