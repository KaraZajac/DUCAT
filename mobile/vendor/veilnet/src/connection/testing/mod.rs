//! Testing utilities for veilnet connections.

use std::sync::Arc;

use async_trait::async_trait;
use veilid_core::{
    CryptoKind, DHTRecordDescriptor, DHTRecordReport, DHTReportScope, DHTSchema, KeyPair,
    PublicKey, RecordKey, RouteBlob, SetDHTValueOptions, Target, Timestamp, UpdateCallback,
    ValueData, ValueSubkey, ValueSubkeyRangeSet, VeilidAPI, VeilidAPIResult, VeilidConfig,
};

use crate::{
    Connection,
    connection::{API, Result, RoutingContext, UpdateHandler},
};
use veilid_core::{
    Crypto, OperationId, RouteId, Sequencing, Stability, TableStore, VeilidComponentGuard,
    VeilidState,
};

#[derive(Clone)]
pub struct StubConnection {
    pub add_update_handler: Arc<std::sync::Mutex<dyn Fn(Box<dyn UpdateHandler + Send + Sync>)>>,
    pub require_attachment: Arc<tokio::sync::Mutex<dyn Fn() -> Result<()> + Send + 'static>>,
    pub reset: Arc<tokio::sync::Mutex<dyn Fn() -> Result<()> + Send + 'static>>,
    pub close: Arc<std::sync::Mutex<dyn Fn() -> Result<()>>>,

    routing_context: StubRoutingContext,
}

impl StubConnection {
    pub fn new(routing_context: StubRoutingContext) -> Self {
        Self {
            add_update_handler: Arc::new(std::sync::Mutex::new(|_| {
                panic!("unexpected call to add_update_handler")
            })),
            require_attachment: Arc::new(tokio::sync::Mutex::new(|| {
                panic!("unexpected call to require_attachment")
            })),
            reset: Arc::new(tokio::sync::Mutex::new(|| {
                panic!("unexpected call to reset")
            })),
            close: Arc::new(std::sync::Mutex::new(|| panic!("unexpected call to close"))),
            routing_context,
        }
    }
}

unsafe impl Send for StubConnection {}
unsafe impl Sync for StubConnection {}

#[async_trait]
impl Connection for StubConnection {
    fn add_update_handler(&self, handler: Box<dyn UpdateHandler + Send + Sync>) {
        (*(self.add_update_handler.lock().unwrap()))(handler)
    }
    async fn require_attachment(&mut self) -> Result<()> {
        (*(self.require_attachment.lock().await))()
    }
    fn routing_context(&self) -> impl RoutingContext {
        self.routing_context.clone()
    }
    async fn reset(&mut self) -> Result<()> {
        (*(self.reset.lock().await))()
    }
    async fn close(self) -> Result<()> {
        (*(self.close.lock().unwrap()))()
    }
}

#[derive(Clone)]
pub struct StubRoutingContext {
    api: StubAPI,

    pub app_call: Arc<
        tokio::sync::Mutex<dyn Fn(Target, Vec<u8>) -> VeilidAPIResult<Vec<u8>> + Send + 'static>,
    >,
    pub app_message:
        Arc<tokio::sync::Mutex<dyn Fn(Target, Vec<u8>) -> VeilidAPIResult<()> + Send + 'static>>,
    pub get_dht_record_key: Arc<
        std::sync::Mutex<
            dyn Fn(DHTSchema, PublicKey, Option<CryptoKind>) -> VeilidAPIResult<RecordKey>,
        >,
    >,
    pub close_dht_record:
        Arc<tokio::sync::Mutex<dyn Fn(RecordKey) -> VeilidAPIResult<()> + Send + 'static>>,
    pub delete_dht_record:
        Arc<tokio::sync::Mutex<dyn Fn(RecordKey) -> VeilidAPIResult<()> + Send + 'static>>,
    pub get_dht_value: Arc<
        tokio::sync::Mutex<
            dyn Fn(RecordKey, ValueSubkey, bool) -> VeilidAPIResult<Option<ValueData>>
                + Send
                + 'static,
        >,
    >,
    pub set_dht_value: Arc<
        tokio::sync::Mutex<
            dyn Fn(
                    RecordKey,
                    ValueSubkey,
                    Vec<u8>,
                    Option<SetDHTValueOptions>,
                ) -> VeilidAPIResult<Option<ValueData>>
                + Send
                + 'static,
        >,
    >,
    pub watch_dht_values: Arc<
        tokio::sync::Mutex<
            dyn Fn(
                    RecordKey,
                    Option<ValueSubkeyRangeSet>,
                    Option<Timestamp>,
                    Option<u32>,
                ) -> VeilidAPIResult<bool>
                + Send
                + 'static,
        >,
    >,
    pub cancel_dht_watch: Arc<
        tokio::sync::Mutex<
            dyn Fn(RecordKey, Option<ValueSubkeyRangeSet>) -> VeilidAPIResult<bool>
                + Send
                + 'static,
        >,
    >,
    pub inspect_dht_record: Arc<
        tokio::sync::Mutex<
            dyn Fn(
                    RecordKey,
                    Option<ValueSubkeyRangeSet>,
                    DHTReportScope,
                ) -> VeilidAPIResult<DHTRecordReport>
                + Send
                + 'static,
        >,
    >,
}

unsafe impl Send for StubRoutingContext {}
unsafe impl Sync for StubRoutingContext {}

impl StubRoutingContext {
    pub fn new(api: StubAPI) -> Self {
        Self {
            api,
            app_call: Arc::new(tokio::sync::Mutex::new(|_, _| {
                panic!("unexpected call to app_call")
            })),
            app_message: Arc::new(tokio::sync::Mutex::new(|_, _| {
                panic!("unexpected call to app_message")
            })),
            get_dht_record_key: Arc::new(std::sync::Mutex::new(|_, _, _| {
                panic!("unexpected call to get_dht_record_key")
            })),
            close_dht_record: Arc::new(tokio::sync::Mutex::new(|_| {
                panic!("unexpected call to close_dht_record")
            })),
            delete_dht_record: Arc::new(tokio::sync::Mutex::new(|_| {
                panic!("unexpected call to delete_dht_record")
            })),
            get_dht_value: Arc::new(tokio::sync::Mutex::new(|_, _, _| {
                panic!("unexpected call to get_dht_value")
            })),
            set_dht_value: Arc::new(tokio::sync::Mutex::new(|_, _, _, _| {
                panic!("unexpected call to set_dht_value")
            })),
            watch_dht_values: Arc::new(tokio::sync::Mutex::new(|_, _, _, _| {
                panic!("unexpected call to watch_dht_values")
            })),
            cancel_dht_watch: Arc::new(tokio::sync::Mutex::new(|_, _| {
                panic!("unexpected call to cancel_dht_watch")
            })),
            inspect_dht_record: Arc::new(tokio::sync::Mutex::new(|_, _, _| {
                panic!("unexpected call to inspect_dht_record")
            })),
        }
    }
}

#[async_trait]
impl RoutingContext for StubRoutingContext {
    fn api(&self) -> impl API {
        self.api.clone()
    }

    async fn app_call(&self, target: Target, message: Vec<u8>) -> VeilidAPIResult<Vec<u8>> {
        (*(self.app_call.lock().await))(target, message)
    }
    async fn app_message(&self, target: Target, message: Vec<u8>) -> VeilidAPIResult<()> {
        (*(self.app_message.lock().await))(target, message)
    }
    async fn create_dht_record(
        &self,
        kind: CryptoKind,
        schema: DHTSchema,
        owner: Option<KeyPair>,
    ) -> VeilidAPIResult<DHTRecordDescriptor> {
        // Because there's no way to instantiate a DHTRecordDescriptor outside
        // of veilid_core, we'll have to call the real underlying test API
        // instance.
        self.api
            .veilid_api
            .routing_context()?
            .create_dht_record(kind, schema, owner)
            .await
    }
    async fn open_dht_record(
        &self,
        key: RecordKey,
        default_writer: Option<KeyPair>,
    ) -> VeilidAPIResult<DHTRecordDescriptor> {
        // Because there's no way to instantiate a DHTRecordDescriptor outside
        // of veilid_core, we'll have to call the real underlying test API
        // instance.
        self.api
            .veilid_api
            .routing_context()?
            .open_dht_record(key, default_writer)
            .await
    }
    async fn close_dht_record(&self, key: RecordKey) -> VeilidAPIResult<()> {
        (*(self.close_dht_record.lock().await))(key)
    }
    async fn delete_dht_record(&self, key: RecordKey) -> VeilidAPIResult<()> {
        (*(self.delete_dht_record.lock().await))(key)
    }
    async fn get_dht_value(
        &self,
        key: RecordKey,
        subkey: ValueSubkey,
        force_refresh: bool,
    ) -> VeilidAPIResult<Option<ValueData>> {
        (*(self.get_dht_value.lock().await))(key, subkey, force_refresh)
    }
    async fn set_dht_value(
        &self,
        key: RecordKey,
        subkey: ValueSubkey,
        data: Vec<u8>,
        options: Option<SetDHTValueOptions>,
    ) -> VeilidAPIResult<Option<ValueData>> {
        (*(self.set_dht_value.lock().await))(key, subkey, data, options)
    }
    async fn watch_dht_values(
        &self,
        key: RecordKey,
        subkeys: Option<ValueSubkeyRangeSet>,
        expiration: Option<Timestamp>,
        count: Option<u32>,
    ) -> VeilidAPIResult<bool> {
        (*(self.watch_dht_values.lock().await))(key, subkeys, expiration, count)
    }
    async fn cancel_dht_watch(
        &self,
        key: RecordKey,
        subkeys: Option<ValueSubkeyRangeSet>,
    ) -> VeilidAPIResult<bool> {
        (*(self.cancel_dht_watch.lock().await))(key, subkeys)
    }
    async fn inspect_dht_record(
        &self,
        key: RecordKey,
        subkeys: Option<ValueSubkeyRangeSet>,
        scope: DHTReportScope,
    ) -> VeilidAPIResult<DHTRecordReport> {
        (*(self.inspect_dht_record.lock().await))(key, subkeys, scope)
    }
}

#[derive(Clone)]
pub struct StubAPI {
    veilid_api: VeilidAPI,

    pub shutdown: Arc<tokio::sync::Mutex<dyn Fn() + Send + 'static>>,
    pub is_shutdown: Arc<std::sync::Mutex<dyn Fn() -> bool>>,
    pub config: Arc<std::sync::Mutex<dyn Fn() -> VeilidAPIResult<Arc<VeilidConfig>>>>,
    pub get_state:
        Arc<tokio::sync::Mutex<dyn Fn() -> VeilidAPIResult<VeilidState> + Send + 'static>>,
    pub attach: Arc<tokio::sync::Mutex<dyn Fn() -> VeilidAPIResult<()> + Send + 'static>>,
    pub detach: Arc<tokio::sync::Mutex<dyn Fn() -> VeilidAPIResult<()> + Send + 'static>>,
    pub routing_context: Arc<std::sync::Mutex<dyn Fn() -> VeilidAPIResult<StubRoutingContext>>>,
    pub parse_as_target: Arc<std::sync::Mutex<dyn Fn(String) -> VeilidAPIResult<Target>>>,
    pub new_private_route:
        Arc<tokio::sync::Mutex<dyn Fn() -> VeilidAPIResult<RouteBlob> + Send + 'static>>,
    pub new_custom_private_route: Arc<
        tokio::sync::Mutex<
            dyn Fn(Vec<CryptoKind>, Stability, Sequencing) -> VeilidAPIResult<RouteBlob>
                + Send
                + 'static,
        >,
    >,
    pub import_remote_private_route:
        Arc<std::sync::Mutex<dyn Fn(Vec<u8>) -> VeilidAPIResult<RouteId>>>,
    pub release_private_route: Arc<std::sync::Mutex<dyn Fn(RouteId) -> VeilidAPIResult<()>>>,
    pub app_call_reply: Arc<
        tokio::sync::Mutex<dyn Fn(OperationId, Vec<u8>) -> VeilidAPIResult<()> + Send + 'static>,
    >,
}

unsafe impl Send for StubAPI {}
unsafe impl Sync for StubAPI {}

impl StubAPI {
    pub fn new(veilid_api: VeilidAPI) -> Self {
        Self {
            veilid_api,

            shutdown: Arc::new(tokio::sync::Mutex::new(|| {
                panic!("unexpected call to shutdown")
            })),
            is_shutdown: Arc::new(std::sync::Mutex::new(|| {
                panic!("unexpected call to is_shutdown")
            })),
            config: Arc::new(std::sync::Mutex::new(|| {
                panic!("unexpected call to config")
            })),
            get_state: Arc::new(tokio::sync::Mutex::new(|| {
                panic!("unexpected call to get_state")
            })),
            attach: Arc::new(tokio::sync::Mutex::new(|| {
                panic!("unexpected call to attach")
            })),
            detach: Arc::new(tokio::sync::Mutex::new(|| {
                panic!("unexpected call to detach")
            })),
            routing_context: Arc::new(std::sync::Mutex::new(|| {
                panic!("unexpected call to routing_context")
            })),
            parse_as_target: Arc::new(std::sync::Mutex::new(|_| {
                panic!("unexpected call to parse_as_target")
            })),
            new_private_route: Arc::new(tokio::sync::Mutex::new(|| {
                panic!("unexpected call to new_private_route")
            })),
            new_custom_private_route: Arc::new(tokio::sync::Mutex::new(|_, _, _| {
                panic!("unexpected call to new_custom_private_route")
            })),
            import_remote_private_route: Arc::new(std::sync::Mutex::new(|_| {
                panic!("unexpected call to import_remote_private_route")
            })),
            release_private_route: Arc::new(std::sync::Mutex::new(|_| {
                panic!("unexpected call to release_private_route")
            })),
            app_call_reply: Arc::new(tokio::sync::Mutex::new(|_, _| {
                panic!("unexpected call to app_call_reply")
            })),
        }
    }
}

#[async_trait]
impl API for StubAPI {
    async fn shutdown(self) {
        (*(self.shutdown.lock().await))()
    }
    fn is_shutdown(&self) -> bool {
        (*(self.is_shutdown.lock().unwrap()))()
    }
    fn config(&self) -> VeilidAPIResult<Arc<VeilidConfig>> {
        (*(self.config.lock().unwrap()))()
    }
    fn crypto<'a>(&'a self) -> VeilidAPIResult<VeilidComponentGuard<'a, Crypto>> {
        self.veilid_api.crypto()
    }
    fn table_store<'a>(&'a self) -> VeilidAPIResult<VeilidComponentGuard<'a, TableStore>> {
        self.veilid_api.table_store()
    }
    async fn get_state(&self) -> VeilidAPIResult<VeilidState> {
        (*(self.get_state.lock().await))()
    }
    async fn attach(&self) -> VeilidAPIResult<()> {
        (*(self.attach.lock().await))()
    }
    async fn detach(&self) -> VeilidAPIResult<()> {
        (*(self.detach.lock().await))()
    }
    fn routing_context(&self) -> VeilidAPIResult<impl RoutingContext> {
        (*(self.routing_context.lock().unwrap()))()
    }
    async fn new_private_route(&self) -> VeilidAPIResult<RouteBlob> {
        (*(self.new_private_route.lock().await))()
    }
    async fn new_custom_private_route(
        &self,
        crypto_kinds: &[CryptoKind],
        stability: Stability,
        sequencing: Sequencing,
    ) -> VeilidAPIResult<RouteBlob> {
        (*(self.new_custom_private_route.lock().await))(
            crypto_kinds.to_vec(),
            stability,
            sequencing,
        )
    }
    fn import_remote_private_route(&self, blob: Vec<u8>) -> VeilidAPIResult<RouteId> {
        (*(self.import_remote_private_route.lock().unwrap()))(blob)
    }
    fn release_private_route(&self, route_id: RouteId) -> VeilidAPIResult<()> {
        (*(self.release_private_route.lock().unwrap()))(route_id)
    }
    async fn app_call_reply(&self, call_id: OperationId, message: Vec<u8>) -> VeilidAPIResult<()> {
        (*(self.app_call_reply.lock().await))(call_id, message)
    }
}

pub async fn create_test_api(temp_dir: &std::path::Path) -> anyhow::Result<VeilidAPI> {
    let update_callback: UpdateCallback = Arc::new(|_| {});

    let config = create_test_config(temp_dir);

    Ok(veilid_core::api_startup(update_callback, config).await?)
}

pub fn create_test_config(temp_dir: &std::path::Path) -> VeilidConfig {
    // Generate a random 32-character string using system time and thread ID
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::SystemTime;

    let mut hasher = DefaultHasher::new();
    SystemTime::now().hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);

    let hash = hasher.finish();
    let namespace = format!("{:032x}", hash);

    VeilidConfig {
        program_name: "veilnet_test".to_string(),
        namespace,
        capabilities: veilid_core::VeilidConfigCapabilities { disable: vec![] },

        table_store: veilid_core::VeilidConfigTableStore {
            directory: temp_dir.join("table_store").to_string_lossy().to_string(),
            delete: true,
            ..Default::default()
        },

        block_store: veilid_core::VeilidConfigBlockStore {
            directory: temp_dir.join("block_store").to_string_lossy().to_string(),
            delete: true,
        },

        protected_store: veilid_core::VeilidConfigProtectedStore {
            allow_insecure_fallback: true,
            always_use_insecure_storage: true,
            directory: temp_dir
                .join("protected_store")
                .to_string_lossy()
                .to_string(),
            delete: true,
            device_encryption_key_password: "".to_string(),
            new_device_encryption_key_password: None,
        },

        network: veilid_core::VeilidConfigNetwork {
            routing_table: veilid_core::VeilidConfigRoutingTable {
                ..Default::default()
            },

            rpc: veilid_core::VeilidConfigRPC {
                ..Default::default()
            },

            dht: veilid_core::VeilidConfigDHT {
                ..Default::default()
            },

            tls: veilid_core::VeilidConfigTLS {
                ..Default::default()
            },
            privacy: veilid_core::VeilidConfigPrivacy {
                require_inbound_relay: false,
            },

            protocol: veilid_core::VeilidConfigProtocol {
                ..Default::default()
            },

            ..Default::default()
        },

        internal: None,
    }
}

#[cfg(all(test, feature = "testing"))]
mod tests {
    use tempfile::tempdir;
    use veilid_core::{BarePublicKey, BareRecordKey, CRYPTO_KIND_VLD0, DHTSchemaDFLT, RecordKey};

    use super::*;

    #[tokio::test]
    async fn test_api() {
        let temp_dir = tempdir().unwrap();
        let veilid_api = create_test_api(temp_dir.path()).await.expect("test api");

        // Test that we can get components
        let crypto_result = veilid_api.crypto();
        assert!(crypto_result.is_ok(), "Failed to get crypto component");

        let table_store_result = veilid_api.table_store();
        assert!(
            table_store_result.is_ok(),
            "Failed to get table_store component"
        );

        // Test that components are functional
        let crypto = crypto_result.unwrap();
        let table_store = table_store_result.unwrap();

        // Basic crypto operations
        let key_pair = crypto.get(CRYPTO_KIND_VLD0).unwrap().generate_keypair();
        assert!(key_pair.key().value().first_nonzero_bit().is_some());

        // Basic table store operations
        let db_result = table_store.open("test", 1).await;
        assert!(db_result.is_ok(), "Failed to open table db");

        veilid_api.shutdown().await
    }

    #[tokio::test]
    async fn test_create_open_dht() {
        let temp_dir = tempdir().unwrap();
        let veilid_api = create_test_api(temp_dir.path()).await.expect("test api");
        let api = StubAPI::new(veilid_api);
        let mut routing_context = StubRoutingContext::new(api);

        let created_dht_rec = routing_context
            .create_dht_record(
                CRYPTO_KIND_VLD0,
                DHTSchema::DFLT(DHTSchemaDFLT::new(1).expect("dht schema")),
                None,
            )
            .await
            .expect("created a dht");
        assert!(
            created_dht_rec
                .key()
                .value()
                .key()
                .first_nonzero_bit()
                .is_some()
        );

        routing_context.get_dht_value =
            Arc::new(tokio::sync::Mutex::new(|_key, _subkey, _force_refresh| {
                Ok(Some(
                    ValueData::new(
                        b"foo".to_vec(),
                        PublicKey::new(CRYPTO_KIND_VLD0, BarePublicKey::default()),
                    )
                    .unwrap(),
                ))
            }));

        assert_eq!(
            routing_context
                .get_dht_value(
                    RecordKey::new(CRYPTO_KIND_VLD0, BareRecordKey::default()),
                    0,
                    true
                )
                .await
                .unwrap(),
            Some(
                ValueData::new(
                    b"foo".to_vec(),
                    PublicKey::new(CRYPTO_KIND_VLD0, BarePublicKey::default())
                )
                .unwrap()
            ),
        );

        let opened_dht_rec = routing_context
            .open_dht_record(created_dht_rec.key().clone(), None)
            .await
            .expect("opened a dht");
        assert!(
            opened_dht_rec
                .key()
                .value()
                .key()
                .first_nonzero_bit()
                .is_some()
        );
        assert_eq!(opened_dht_rec.key(), created_dht_rec.key());
    }
}
