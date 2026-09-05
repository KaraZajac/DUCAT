use crate::attachment_manager::{AttachmentManager, AttachmentManagerStartupContext};
use crate::crypto::Crypto;
use crate::logging::*;
use crate::network_manager::{NetworkManager, NetworkManagerStartupContext};
use crate::routing_table::{RoutingTable, RoutingTableStartupContext};
use crate::rpc_processor::{RPCProcessor, RPCProcessorStartupContext};
use crate::storage_manager::StorageManager;
use crate::veilid_api::*;
use crate::veilid_config::*;
use crate::*;

impl_veilid_log_facility!("corectx");

/// Callback the application registers to receive `VeilidUpdate` events from a running node.
pub type UpdateCallback = Arc<dyn Fn(VeilidUpdate) + Send + Sync>;

type InitKey = (String, String);

/// Convert an eyre::Report into a VeilidAPIError, preserving typed variants
/// when possible. Falls back to `Internal` for unknown error chains.
fn eyre_to_veilid_api_error(report: eyre::Report) -> VeilidAPIError {
    match report.downcast::<VeilidAPIError>() {
        Ok(api_err) => api_err,
        Err(report) => VeilidAPIError::internal(report),
    }
}

/////////////////////////////////////////////////////////////////////////////
#[derive(Clone, Debug)]
pub(crate) struct VeilidCoreContext {
    registry: VeilidComponentRegistry,
}

impl_veilid_component_accessors!(VeilidCoreContext);

impl VeilidCoreContext {
    #[cfg_attr(
        feature = "instrument",
        instrument(
            level = "trace",
            target = "core_context",
            err,
            skip_all,
            fields(__VEILID_LOG_KEY)
        )
    )]
    async fn new_with_config(
        update_callback: UpdateCallback,
        config: VeilidConfig,
    ) -> VeilidAPIResult<VeilidCoreContext> {
        #[cfg(feature = "instrument")]
        tracing::Span::current().record(
            "__VEILID_LOG_KEY",
            VeilidLayerFilter::make_veilid_log_key(&config.program_name, &config.namespace),
        );

        // Set up config from json
        let config = VeilidStartupOptions::try_new(config, update_callback)?;

        Self::new_common(config).await
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(
            level = "trace",
            target = "core_context",
            err,
            skip_all,
            fields(__VEILID_LOG_KEY)
        )
    )]
    async fn new_common(
        startup_options: VeilidStartupOptions,
    ) -> VeilidAPIResult<VeilidCoreContext> {
        cfg_if! {
            if #[cfg(target_os = "android")] {
                if !crate::veilid_api::android::is_android_ready() {
                    apibail_internal!("Android globals are not set up");
                }
            }
        }

        let (program_name, namespace, update_callback) = {
            let cfginner = startup_options.config();
            (
                cfginner.program_name.clone(),
                cfginner.namespace.clone(),
                startup_options.update_callback(),
            )
        };

        let log_key = VeilidLayerFilter::make_veilid_log_key(&program_name, &namespace).to_string();

        #[cfg(feature = "instrument")]
        tracing::Span::current().record("__VEILID_LOG_KEY", log_key.clone());
        ApiTracingLayer::add_callback(log_key.clone(), update_callback.clone())?;

        // Create component registry
        let registry = VeilidComponentRegistry::new(startup_options);

        // Warn if internal "footgun" tuning was provided without the footgun-config feature
        #[cfg(not(feature = "footgun-config"))]
        if registry
            .config()
            .internal
            .as_ref()
            .is_some_and(|i| *i != VeilidConfigInternal::default())
        {
            veilid_log!(registry warn "VeilidConfig.internal footgun tuning was provided but the 'footgun-config' feature is not enabled; ignoring it and using built-in defaults");
        }

        veilid_log!(registry info "Veilid API starting up");
        if let Some(target) = option_env!("TARGET") {
            veilid_log!(registry info     "Build Target: {}", target);
        }
        veilid_log!(registry info     "Program Name: {}", program_name);
        if !namespace.is_empty() {
            veilid_log!(registry info "Namespace:    {}", namespace);
        }
        veilid_log!(registry info     "Features:     {:?}", veilid_features());
        veilid_log!(registry info     "Version:      {}", veilid_version_string());
        #[cfg(feature = "footgun-nodeid-target")]
        {
            veilid_log!(registry warn
                "Footgun feature is enabled. This disables sender privacy protections and should be avoided in production.");
        }

        // Register all components
        registry.register(ProtectedStore::new);
        registry.register(Crypto::new);
        registry.register(TableStore::new);
        #[cfg(feature = "unstable-blockstore")]
        registry.register(BlockStore::new);
        registry.register_with_context(RoutingTable::new, RoutingTableStartupContext::default());
        registry.register(StorageManager::new);
        registry
            .register_with_context(NetworkManager::new, NetworkManagerStartupContext::default());
        registry.register_with_context(RPCProcessor::new, RPCProcessorStartupContext::default());
        registry.register_with_context(
            AttachmentManager::new,
            AttachmentManagerStartupContext::default(),
        );

        // Run initialization
        // This should make the majority of subsystems functional
        if let Err(e) = registry.init().await {
            ApiTracingLayer::remove_callback(log_key.clone())?;
            return Err(eyre_to_veilid_api_error(e));
        }
        // Run post-initialization
        // This should resolve any inter-subsystem dependencies
        // required for background processes that utilize multiple subsystems
        // Background processes also often require registry lookup of the
        // current subsystem, which is not available until after init succeeds
        // This is where the attachment manager starts the background tick
        if let Err(e) = registry.post_init().await {
            registry.terminate().await;
            ApiTracingLayer::remove_callback(log_key)?;
            return Err(eyre_to_veilid_api_error(e));
        }

        veilid_log!(registry info "Veilid API startup complete");

        Ok(Self { registry })
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "core_context", skip_all, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn shutdown(self) {
        veilid_log!(self info "Veilid API shutting down");

        let config = self.registry.config();
        let program_name = &config.program_name;
        let namespace = &config.namespace;
        let update_callback = self.registry().update_callback();

        // Run pre-termination
        // This should shut down background processes that may require the existence of
        // other subsystems that may not exist during final termination
        self.registry.pre_terminate().await;

        // Run termination
        // This should finish any shutdown operations for the subsystems
        self.registry.terminate().await;

        veilid_log!(self info "Veilid API shutdown complete");

        let log_key = VeilidLayerFilter::make_veilid_log_key(program_name, namespace).to_string();
        if let Err(e) = ApiTracingLayer::remove_callback(log_key) {
            error!("Error removing callback from ApiTracingLayer: {}", e);
        }

        // send final shutdown update
        update_callback(VeilidUpdate::Shutdown);
    }
}

/////////////////////////////////////////////////////////////////////////////

pub(crate) trait RegisteredComponents {
    fn protected_store<'a>(&self) -> VeilidComponentGuard<'a, ProtectedStore>;
    fn crypto<'a>(&self) -> VeilidComponentGuard<'a, Crypto>;
    fn table_store<'a>(&self) -> VeilidComponentGuard<'a, TableStore>;
    fn storage_manager<'a>(&self) -> VeilidComponentGuard<'a, StorageManager>;
    fn routing_table<'a>(&self) -> VeilidComponentGuard<'a, RoutingTable>;
    fn network_manager<'a>(&self) -> VeilidComponentGuard<'a, NetworkManager>;
    fn rpc_processor<'a>(&self) -> VeilidComponentGuard<'a, RPCProcessor>;
    fn attachment_manager<'a>(&self) -> VeilidComponentGuard<'a, AttachmentManager>;
}

impl<T: VeilidComponentRegistryAccessor + ?Sized> RegisteredComponents for T {
    fn protected_store<'a>(&self) -> VeilidComponentGuard<'a, ProtectedStore> {
        self.registry().lookup::<ProtectedStore>().unwrap_or_log()
    }
    fn crypto<'a>(&self) -> VeilidComponentGuard<'a, Crypto> {
        self.registry().lookup::<Crypto>().unwrap_or_log()
    }
    fn table_store<'a>(&self) -> VeilidComponentGuard<'a, TableStore> {
        self.registry().lookup::<TableStore>().unwrap_or_log()
    }
    fn storage_manager<'a>(&self) -> VeilidComponentGuard<'a, StorageManager> {
        self.registry().lookup::<StorageManager>().unwrap_or_log()
    }
    fn routing_table<'a>(&self) -> VeilidComponentGuard<'a, RoutingTable> {
        self.registry().lookup::<RoutingTable>().unwrap_or_log()
    }
    fn network_manager<'a>(&self) -> VeilidComponentGuard<'a, NetworkManager> {
        self.registry().lookup::<NetworkManager>().unwrap_or_log()
    }
    fn rpc_processor<'a>(&self) -> VeilidComponentGuard<'a, RPCProcessor> {
        self.registry().lookup::<RPCProcessor>().unwrap_or_log()
    }
    fn attachment_manager<'a>(&self) -> VeilidComponentGuard<'a, AttachmentManager> {
        self.registry()
            .lookup::<AttachmentManager>()
            .unwrap_or_log()
    }
}

/////////////////////////////////////////////////////////////////////////////

lazy_static::lazy_static! {
    static ref INITIALIZED: Mutex<HashSet<InitKey>> = Mutex::new(HashSet::new());
    static ref STARTUP_TABLE: AsyncTagLockTable<InitKey> = AsyncTagLockTable::new();
}

/// Initialize a Veilid node, with the configuration in JSON format.
///
/// Must be called only once per 'program_name + namespace' combination at the start of an application.
/// The 'config_json' must specify a unique 'program_name + namespace' combination per simulataneous call to api_startup.
/// You can use the same program_name multiple times to create separate storage locations.
/// Multiple namespaces for the same program_name will use the same databases and on-disk locations, but will partition keys internally
/// to keep the namespaces distict.
///
/// * `update_callback` - called when internal state of the Veilid node changes, for example, when app-level messages are received, when private routes die and need to be reallocated, or when routing table states change.
/// * `config_json` - called at startup to supply a JSON configuration object.
///
/// Returns a [VeilidAPI] object that can be used to operate the node. Errors with
/// `VeilidAPIError::AlreadyInitialized` if a node is already running for the same
/// `program_name + namespace`; the previous [VeilidAPI] must be shut down first.
/// Errors with `VeilidAPIError::Generic` if `config_json` is not valid JSON, or if the
/// parsed config fails validation (empty `program_name`, a `program_name`/`namespace` that is
/// not a valid filename, out-of-range connection caps, or invalid RPC/DHT tuning). Errors with
/// `VeilidAPIError::Internal` if subsystem init or post-init fails (or, on Android, if the
/// Android globals were not set up); init/post-init may also surface a more specific
/// `VeilidAPIError` variant from the failing subsystem.
///
/// Blocks until subsystems are initialized (disk stores opened, background tasks started);
/// the network is not bound until the node attaches. Startup/shutdown is serialized per
/// `program_name + namespace`.
/// [VeilidAPI] is reference-counted: dropping the last clone spawns a detached shutdown that
/// releases the `program_name + namespace` slot. Call [VeilidAPI::shutdown] to shut down
/// deterministically and wait for completion.
pub async fn api_startup_json(
    update_callback: UpdateCallback,
    config_json: String,
) -> VeilidAPIResult<VeilidAPI> {
    // Parse the JSON config, collecting any keys that don't map to a known config field
    // (likely a typo or a setting that moved under 'internal'). We warn about these once the
    // node is up rather than erroring, so an unknown key never blocks startup.
    let mut unknown_keys = Vec::<String>::new();
    let config: VeilidConfig = {
        let mut de = serde_json::Deserializer::from_str(&config_json);
        let config = serde_ignored::deserialize(&mut de, |path| {
            unknown_keys.push(path.to_string());
        })
        .map_err(VeilidAPIError::generic)?;
        de.end().map_err(VeilidAPIError::generic)?;
        config
    };

    let veilid_api = api_startup(update_callback, config).await?;

    // Now that the node (and its logging) is up, surface any unrecognized config keys
    if !unknown_keys.is_empty() {
        let registry = veilid_api.core_context()?.registry();
        for key in &unknown_keys {
            veilid_log!(registry warn "ignoring unknown veilid config key: '{}'", key);
        }
    }

    Ok(veilid_api)
}

/// Initialize a Veilid node, with the configuration object.
///
/// Must be called only once at the start of an application.
///
/// * `update_callback` - called when internal state of the Veilid node changes, for example, when app-level messages are received, when private routes die and need to be reallocated, or when routing table states change.
/// * `config` - called at startup to supply a configuration object.
///
/// Returns a [VeilidAPI] object that can be used to operate the node. Errors with
/// `VeilidAPIError::AlreadyInitialized` if a node is already running for the same
/// `program_name + namespace`; the previous [VeilidAPI] must be shut down first.
/// Errors with `VeilidAPIError::Generic` if `config` fails validation (empty `program_name`,
/// a `program_name`/`namespace` that is not a valid filename, out-of-range connection caps,
/// or invalid RPC/DHT tuning). Errors with `VeilidAPIError::Internal` if subsystem init or
/// post-init fails (or, on Android, if the Android globals were not set up); init/post-init
/// may also surface a more specific `VeilidAPIError` variant from the failing subsystem.
///
/// Blocks until subsystems are initialized (disk stores opened, background tasks started);
/// the network is not bound until the node attaches. Startup/shutdown is serialized per
/// `program_name + namespace`.
/// [VeilidAPI] is reference-counted: dropping the last clone spawns a detached shutdown that
/// releases the `program_name + namespace` slot. Call [VeilidAPI::shutdown] to shut down
/// deterministically and wait for completion.
#[cfg_attr(
    feature = "instrument",
    instrument(
        level = "trace",
        target = "core_context",
        err,
        skip_all,
        fields(__VEILID_LOG_KEY)
    )
)]
pub async fn api_startup(
    update_callback: UpdateCallback,
    config: VeilidConfig,
) -> VeilidAPIResult<VeilidAPI> {
    #[cfg(feature = "debug-locks")]
    veilid_tools::deadlock_detector::start_deadlock_detector();

    // Get the program_name and namespace we're starting up in
    let program_name = config.program_name.clone();
    let namespace = config.namespace.clone();

    #[cfg(feature = "instrument")]
    tracing::Span::current().record(
        "__VEILID_LOG_KEY",
        VeilidLayerFilter::make_veilid_log_key(&program_name, &namespace),
    );

    let init_key = (program_name, namespace);

    // Only allow one startup/shutdown per program_name+namespace combination simultaneously
    let _tag_guard = STARTUP_TABLE.lock_tag(init_key.clone()).await;
    // See if we have an API started up already
    if INITIALIZED.lock().contains(&init_key) {
        apibail_already_initialized!();
    }

    // Create core context
    let context = VeilidCoreContext::new_with_config(update_callback, config).await?;

    // Return an API object around our context
    let veilid_api = VeilidAPI::new(context);

    // Add to the initialized set
    INITIALIZED.lock().insert(init_key);

    Ok(veilid_api)
}

#[cfg_attr(
    feature = "instrument",
    instrument(
        level = "trace",
        target = "core_context",
        skip_all,
        fields(__VEILID_LOG_KEY = context.log_key())
    )
)]
pub(crate) async fn api_shutdown(context: VeilidCoreContext) {
    let init_key = {
        let registry = context.registry();
        let config = registry.config();
        (config.program_name.clone(), config.namespace.clone())
    };

    // Only allow one startup/shutdown per program_name+namespace combination simultaneously
    let _tag_guard = STARTUP_TABLE.lock_tag(init_key.clone()).await;

    // See if we have an API started up already
    if !INITIALIZED.lock().contains(&init_key) {
        return;
    }

    // Shutdown the context
    context.shutdown().await;

    // Remove from the initialized set
    INITIALIZED.lock().remove(&init_key);
}
