use crate::*;

impl_veilid_log_facility!("bstore");

struct BlockStoreInner {
    //
}

impl fmt::Debug for BlockStoreInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlockStoreInner").finish()
    }
}

#[derive(Debug)]
#[must_use]
pub struct BlockStore {
    registry: VeilidComponentRegistry,
    inner: Mutex<BlockStoreInner>,
}

impl_veilid_component!(BlockStore);

impl BlockStore {
    fn new_inner() -> BlockStoreInner {
        BlockStoreInner {}
    }
    pub(crate) fn new(registry: VeilidComponentRegistry) -> Self {
        Self {
            registry,
            inner: Mutex::new(Self::new_inner()),
        }
    }

    fn log_facilities_impl(&self) -> VeilidComponentLogFacilities {
        VeilidComponentLogFacilities::new()
            .with_facility(VeilidComponentLogFacility::try_new_enabled("bstore").unwrap())
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    async fn init_async(&self) -> EyreResult<()> {
        Ok(())
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip(self), err, fields(__VEILID_LOG_KEY = self.log_key())))]
    async fn post_init_async(&self) -> EyreResult<()> {
        Ok(())
    }

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    async fn pre_terminate_async(&self) {}

    #[cfg_attr(feature = "instrument", instrument(level = "debug", skip(self), fields(__VEILID_LOG_KEY = self.log_key())))]
    async fn terminate_async(&self) {}
}
