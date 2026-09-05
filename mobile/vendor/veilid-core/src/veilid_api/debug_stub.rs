//! Debug command interface stubs, compiled when the `debug-api` feature is off.

use super::*;

#[allow(clippy::unused_async)]
impl VeilidAPI {
    /// Debug commands are unavailable without the `debug-api` feature; returns an empty string.
    pub fn debug_help(&self) -> VeilidAPIResult<String> {
        Ok(String::new())
    }

    /// Debug commands are unavailable without the `debug-api` feature; returns an empty string.
    pub async fn debug_uptime(&self) -> VeilidAPIResult<String> {
        Ok(String::new())
    }

    /// Debug commands are unavailable without the `debug-api` feature; returns an empty string.
    pub async fn debug_die(
        &self,
        _mode: Option<String>,
        _message: Vec<String>,
    ) -> VeilidAPIResult<String> {
        Ok(String::new())
    }

    /// Debug commands are unavailable without the `debug-api` feature; returns an empty string.
    pub async fn debug(&self, _args: String) -> VeilidAPIResult<String> {
        Ok(String::new())
    }
}
