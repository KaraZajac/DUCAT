use async_trait::async_trait;

#[derive(Clone)]
pub struct RoutingContext(pub(super) veilid_core::RoutingContext);

impl From<veilid_core::RoutingContext> for RoutingContext {
    fn from(routing_context: veilid_core::RoutingContext) -> Self {
        RoutingContext(routing_context)
    }
}

#[async_trait]
impl crate::connection::RoutingContext for RoutingContext {
    fn api(&self) -> impl crate::connection::API {
        super::api::API(self.0.api())
    }

    async fn app_call(
        &self,
        target: veilid_core::Target,
        message: Vec<u8>,
    ) -> veilid_core::VeilidAPIResult<Vec<u8>> {
        self.0.app_call(target, message).await
    }

    async fn app_message(
        &self,
        target: veilid_core::Target,
        message: Vec<u8>,
    ) -> veilid_core::VeilidAPIResult<()> {
        self.0.app_message(target, message).await
    }

    async fn create_dht_record(
        &self,
        kind: veilid_core::CryptoKind,
        schema: veilid_core::DHTSchema,
        owner: Option<veilid_core::KeyPair>,
    ) -> veilid_core::VeilidAPIResult<veilid_core::DHTRecordDescriptor> {
        self.0.create_dht_record(kind, schema, owner).await
    }

    async fn open_dht_record(
        &self,
        key: veilid_core::RecordKey,
        default_writer: Option<veilid_core::KeyPair>,
    ) -> veilid_core::VeilidAPIResult<veilid_core::DHTRecordDescriptor> {
        self.0.open_dht_record(key, default_writer).await
    }

    async fn close_dht_record(
        &self,
        key: veilid_core::RecordKey,
    ) -> veilid_core::VeilidAPIResult<()> {
        self.0.close_dht_record(key).await
    }

    async fn delete_dht_record(
        &self,
        key: veilid_core::RecordKey,
    ) -> veilid_core::VeilidAPIResult<()> {
        self.0.delete_dht_record(key).await
    }

    async fn get_dht_value(
        &self,
        key: veilid_core::RecordKey,
        subkey: veilid_core::ValueSubkey,
        force_refresh: bool,
    ) -> veilid_core::VeilidAPIResult<Option<veilid_core::ValueData>> {
        self.0.get_dht_value(key, subkey, force_refresh).await
    }

    async fn set_dht_value(
        &self,
        key: veilid_core::RecordKey,
        subkey: veilid_core::ValueSubkey,
        data: Vec<u8>,
        options: Option<veilid_core::SetDHTValueOptions>,
    ) -> veilid_core::VeilidAPIResult<Option<veilid_core::ValueData>> {
        self.0.set_dht_value(key, subkey, data, options).await
    }

    async fn watch_dht_values(
        &self,
        key: veilid_core::RecordKey,
        subkeys: Option<veilid_core::ValueSubkeyRangeSet>,
        expiration: Option<veilid_core::Timestamp>,
        count: Option<u32>,
    ) -> veilid_core::VeilidAPIResult<bool> {
        self.0
            .watch_dht_values(key, subkeys, expiration, count)
            .await
    }

    async fn cancel_dht_watch(
        &self,
        key: veilid_core::RecordKey,
        subkeys: Option<veilid_core::ValueSubkeyRangeSet>,
    ) -> veilid_core::VeilidAPIResult<bool> {
        self.0.cancel_dht_watch(key, subkeys).await
    }

    async fn inspect_dht_record(
        &self,
        key: veilid_core::RecordKey,
        subkeys: Option<veilid_core::ValueSubkeyRangeSet>,
        scope: veilid_core::DHTReportScope,
    ) -> veilid_core::VeilidAPIResult<veilid_core::DHTRecordReport> {
        self.0.inspect_dht_record(key, subkeys, scope).await
    }
}
