//! Connection management for the Veilid network.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, thread_rng};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use tempfile::TempDir;
use tokio::sync::watch;
use tracing::instrument;
use veilid_core::{VeilidConfig, VeilidStateAttachment, VeilidUpdate};

use crate::connection::{
    HandlerChain, Result, StateAttachmentWatcher, UpdateDispatch, UpdateHandler,
};

/// A connection to the Veilid network.
///
/// Provides access to Veilid's routing context and manages network attachment state.
/// The connection handles automatic state monitoring and provides streams of network updates.
pub struct Connection {
    routing_context: veilid_core::RoutingContext,
    state_dir: Option<TempDir>,
    update_chain: Arc<Mutex<HandlerChain>>,
    attachment_state_rx: watch::Receiver<VeilidStateAttachment>,
    /// DUCAT modification (see STIGMERGE-NOTICE.md): whether this
    /// connection started the node it rides. A borrowed node — an
    /// application's own, shared with its mailbox and boards — must never
    /// be detached, reattached or shut down by the swarm's recovery paths.
    owned: bool,
}

impl Connection {
    /// Creates a new connection with default configuration and temporary storage.
    ///
    /// This is the simplest way to establish a Veilid connection. Storage will be
    /// cleaned up automatically when the connection is dropped.
    pub async fn new() -> Result<Connection> {
        let state_dir = TempDir::new()?;
        let mut conn = Self::new_config(Self::config(state_dir.path(), None)).await?;
        conn.state_dir = Some(state_dir);
        Ok(conn)
    }

    /// Creates a new connection with custom configuration.
    ///
    /// Use this when you need to specify custom storage locations or other
    /// Veilid configuration parameters.
    #[instrument(skip_all)]
    pub async fn new_config(config: VeilidConfig) -> Result<Connection> {
        let update_chain = Arc::new(Mutex::new(HandlerChain::new()));
        let update_source = Arc::new(UpdateDispatch::new(update_chain.clone()));
        let update_cb = Arc::new(move |update: VeilidUpdate| {
            update_source.update(update);
        });
        let api: veilid_core::VeilidAPI = veilid_core::api_startup(update_cb, config).await?;
        api.attach().await?;

        let (attachment_monitor, attachment_state_rx) = StateAttachmentWatcher::new();
        update_chain
            .lock()
            .unwrap()
            .add(Box::new(attachment_monitor));
        Ok(Connection {
            routing_context: api.routing_context()?,
            state_dir: None,
            update_chain,
            attachment_state_rx,
            owned: true,
        })
    }

    /// DUCAT modification (see STIGMERGE-NOTICE.md): ride a node that is
    /// already running — an application that has its own Veilid node must
    /// not start a second one for the swarm.
    ///
    /// Returns the connection and a **feeder**: the host owns the update
    /// callback (it registered it at `api_startup`), so it forwards every
    /// `VeilidUpdate` it receives into the feeder and this connection's
    /// handler chain sees the same stream it would have owned. The current
    /// attachment state is seeded from `get_state()` so `require_attachment`
    /// on an already-attached node returns instead of waiting for the next
    /// state change that may never come.
    pub async fn from_api(
        api: veilid_core::VeilidAPI,
    ) -> Result<(Connection, Box<dyn Fn(VeilidUpdate) + Send + Sync>)> {
        let update_chain = Arc::new(Mutex::new(HandlerChain::new()));
        let update_source = Arc::new(UpdateDispatch::new(update_chain.clone()));
        let (attachment_monitor, attachment_state_rx) = StateAttachmentWatcher::new();
        update_chain
            .lock()
            .unwrap()
            .add(Box::new(attachment_monitor));
        let seed = api.get_state().await?;
        update_source.update(VeilidUpdate::Attachment(seed.attachment));
        let feeder: Box<dyn Fn(VeilidUpdate) + Send + Sync> = {
            let us = update_source.clone();
            Box::new(move |u: VeilidUpdate| us.update(u))
        };
        Ok((
            Connection {
                routing_context: api.routing_context()?,
                state_dir: None,
                update_chain,
                attachment_state_rx,
                owned: false,
            },
            feeder,
        ))
    }

    pub fn config(state_dir: &Path, program_name: Option<String>) -> VeilidConfig {
        VeilidConfig {
            program_name: program_name.unwrap_or("veilnet-app".to_string()),
            namespace: {
                let mut bytes = [0u8; 32];
                thread_rng().fill_bytes(&mut bytes);
                URL_SAFE_NO_PAD.encode(bytes)
            },
            network: veilid_core::VeilidConfigNetwork {
                ..Default::default()
            },
            table_store: veilid_core::VeilidConfigTableStore {
                directory: state_dir.join("table").to_string_lossy().to_string(),
                ..Default::default()
            },
            block_store: veilid_core::VeilidConfigBlockStore {
                directory: state_dir.join("block").to_string_lossy().to_string(),
                ..Default::default()
            },
            protected_store: veilid_core::VeilidConfigProtectedStore {
                allow_insecure_fallback: true,
                always_use_insecure_storage: false,
                directory: state_dir.join("protected").to_string_lossy().to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

#[async_trait]
impl crate::connection::Connection for Connection {
    fn add_update_handler(&self, handler: Box<dyn UpdateHandler + Send + Sync>) {
        self.update_chain.lock().unwrap().add(handler);
    }

    /// Waits until the connection is attached and ready for public internet use.
    ///
    /// This blocks until the Veilid node is fully connected and can communicate
    /// over the public internet. Call this before performing network operations.
    #[instrument(skip_all)]
    async fn require_attachment(&mut self) -> Result<()> {
        let res = self
            .attachment_state_rx
            .wait_for(|attachment| attachment.public_internet_ready)
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Gets the underlying Veilid routing context.
    ///
    /// The routing context provides low-level access to Veilid's DHT and
    /// routing capabilities. Most users should use higher-level abstractions instead.
    fn routing_context(&self) -> impl crate::connection::RoutingContext {
        crate::connection::veilid::routing_context::RoutingContext(self.routing_context.clone())
    }

    /// Resets the connection by detaching and reattaching to the network.
    ///
    /// This can help recover from network issues or routing problems.
    /// The operation will wait for full attachment before returning.
    #[instrument(skip_all)]
    async fn reset(&mut self) -> Result<()> {
        // DUCAT modification: a borrowed node's attachment belongs to its
        // owner — the mailbox poller is riding it too. Recovery on a
        // borrowed connection waits for the owner's node to be well again
        // rather than bouncing it out from under everything else.
        if self.owned {
            self.routing_context.api().detach().await?;
            self.routing_context.api().attach().await?;
        }
        self.require_attachment().await?;
        Ok(())
    }

    /// Closes the connection and cleans up resources.
    ///
    /// This gracefully shuts down the attachment monitor and releases
    /// any temporary storage. The connection cannot be used after closing.
    async fn close(self) -> Result<()> {
        // DUCAT modification: shutting down a borrowed node would take the
        // whole application's network with it.
        if self.owned {
            self.routing_context.api().shutdown().await;
        }
        Ok(())
    }
}

impl Clone for Connection {
    fn clone(&self) -> Self {
        Self {
            // Share the same routing_context and updates stream (broadcast)
            routing_context: self.routing_context.clone(),

            // No state dir to clean up
            state_dir: None,

            update_chain: self.update_chain.clone(),

            // Clone uses the parent's attachment state watch channel.
            attachment_state_rx: self.attachment_state_rx.clone(),

            owned: self.owned,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// DUCAT modification's own proof: a borrowed connection may be closed
    /// and reset without the node underneath noticing — the application
    /// that owns it (its mailbox, its boards) keeps a working API either
    /// way. This is the guard that lets the swarm share the phone's node.
    #[tokio::test]
    async fn from_api_borrows_without_owning() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let api = crate::connection::testing::create_test_api(temp.path())
            .await
            .expect("test api");
        let (conn, feeder) = Connection::from_api(api.clone())
            .await
            .expect("from_api");
        // The feeder path is the host's update callback in miniature.
        feeder(VeilidUpdate::Attachment(
            api.get_state().await.expect("state").attachment,
        ));
        // Closing the borrowed connection must NOT shut the node down.
        use crate::connection::Connection as _;
        conn.close().await.expect("close");
        assert!(
            api.get_state().await.is_ok(),
            "closing a borrowed connection took the owner's node with it"
        );
        api.shutdown().await;
    }

    #[test]
    fn test_namespace_generation() {
        let config = Connection::config(Path::new("/tmp"), None);

        // Verify it's base64 URL-safe without padding
        assert!(
            !config.namespace.contains('+'),
            "Namespace should not contain '+'"
        );
        assert!(
            !config.namespace.contains('/'),
            "Namespace should not contain '/'"
        );
        assert!(
            !config.namespace.contains('='),
            "Namespace should not contain padding '='"
        );

        // Verify length: 32 bytes base64 URL-safe without padding should be 43 chars
        assert_eq!(
            config.namespace.len(),
            43,
            "Namespace should be 43 characters long"
        );

        // Test that multiple calls generate different namespaces
        let config2 = Connection::config(Path::new("/tmp"), None);
        assert_ne!(
            config.namespace, config2.namespace,
            "Each call should generate a different namespace"
        );
    }
}
