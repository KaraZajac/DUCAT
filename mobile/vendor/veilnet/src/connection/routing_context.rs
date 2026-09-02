use async_trait::async_trait;
use veilid_core::{
    CryptoKind, DHTRecordDescriptor, DHTRecordReport, DHTReportScope, DHTSchema, KeyPair,
    RecordKey, SetDHTValueOptions, Target, Timestamp, ValueData, ValueSubkey, ValueSubkeyRangeSet,
    VeilidAPIResult,
};

use crate::connection::API;

// RoutingContext presents an interface similar to veilid_core::RoutingContext
// methods, through a trait that can be wrapped with middleware or mocked
// entirely.
#[async_trait]
pub trait RoutingContext {
    fn api(&self) -> impl API + Send;

    async fn app_call(&self, target: Target, message: Vec<u8>) -> VeilidAPIResult<Vec<u8>>;
    async fn app_message(&self, target: Target, message: Vec<u8>) -> VeilidAPIResult<()>;
    async fn create_dht_record(
        &self,
        kind: CryptoKind,
        schema: DHTSchema,
        owner: Option<KeyPair>,
    ) -> VeilidAPIResult<DHTRecordDescriptor>;
    async fn open_dht_record(
        &self,
        key: RecordKey,
        default_writer: Option<KeyPair>,
    ) -> VeilidAPIResult<DHTRecordDescriptor>;
    async fn close_dht_record(&self, key: RecordKey) -> VeilidAPIResult<()>;
    async fn delete_dht_record(&self, key: RecordKey) -> VeilidAPIResult<()>;
    async fn get_dht_value(
        &self,
        key: RecordKey,
        subkey: ValueSubkey,
        force_refresh: bool,
    ) -> VeilidAPIResult<Option<ValueData>>;
    async fn set_dht_value(
        &self,
        key: RecordKey,
        subkey: ValueSubkey,
        data: Vec<u8>,
        options: Option<SetDHTValueOptions>,
    ) -> VeilidAPIResult<Option<ValueData>>;
    async fn watch_dht_values(
        &self,
        key: RecordKey,
        subkeys: Option<ValueSubkeyRangeSet>,
        expiration: Option<Timestamp>,
        count: Option<u32>,
    ) -> VeilidAPIResult<bool>;
    async fn cancel_dht_watch(
        &self,
        key: RecordKey,
        subkeys: Option<ValueSubkeyRangeSet>,
    ) -> VeilidAPIResult<bool>;
    async fn inspect_dht_record(
        &self,
        key: RecordKey,
        subkeys: Option<ValueSubkeyRangeSet>,
        scope: DHTReportScope,
    ) -> VeilidAPIResult<DHTRecordReport>;
}
