use std::sync::{Arc, Mutex};

use tokio::sync::watch;
use tracing::debug;
use veilid_core::{
    AttachmentState, VeilidAppCall, VeilidAppMessage, VeilidLog, VeilidRouteChange,
    VeilidStateAttachment, VeilidStateConfig, VeilidStateNetwork, VeilidUpdate, VeilidValueChange,
};

/// Handle veilid update callback events.
pub trait UpdateHandler {
    fn log(&self, _log: &VeilidLog) {}
    fn app_message(&self, _message: &VeilidAppMessage) {}
    fn app_call(&self, _call: &VeilidAppCall) {}
    fn state_attachment(&self, _attachment: &VeilidStateAttachment) {}
    fn state_network(&self, _network: &VeilidStateNetwork) {}
    fn state_config(&self, _config: &VeilidStateConfig) {}
    fn route_change(&self, _change: &VeilidRouteChange) {}
    fn value_change(&self, _change: &VeilidValueChange) {}
    fn shutdown(&self) {}
}

/// Dispatch update callback events to an UpdateHandler.
pub struct UpdateDispatch {
    handler: Arc<Mutex<dyn UpdateHandler + Send + Sync>>,
}

impl UpdateDispatch {
    pub fn new(handler: Arc<Mutex<dyn UpdateHandler + Send + Sync>>) -> Self {
        Self { handler }
    }

    pub fn update(&self, update: VeilidUpdate) {
        let handler = self.handler.lock().unwrap();
        match update {
            VeilidUpdate::Log(ref veilid_log) => handler.log(veilid_log),
            VeilidUpdate::AppMessage(ref veilid_app_message) => {
                handler.app_message(veilid_app_message)
            }
            VeilidUpdate::AppCall(ref veilid_app_call) => handler.app_call(veilid_app_call),
            VeilidUpdate::Attachment(ref veilid_state_attachment) => {
                handler.state_attachment(veilid_state_attachment)
            }
            VeilidUpdate::Network(ref veilid_state_network) => {
                handler.state_network(veilid_state_network)
            }
            VeilidUpdate::Config(ref veilid_state_config) => {
                handler.state_config(veilid_state_config)
            }
            VeilidUpdate::RouteChange(ref veilid_route_change) => {
                handler.route_change(veilid_route_change)
            }
            VeilidUpdate::ValueChange(ref veilid_value_change) => {
                handler.value_change(veilid_value_change)
            }
            VeilidUpdate::Shutdown => handler.shutdown(),
        };
    }
}

/// Handler update event callbacks by invoking a chain of handlers.
pub struct HandlerChain {
    handlers: Vec<Box<dyn UpdateHandler + Send + Sync>>,
}

impl Default for HandlerChain {
    fn default() -> Self {
        Self::new()
    }
}

impl HandlerChain {
    pub fn new() -> Self {
        Self { handlers: vec![] }
    }

    pub fn add(&mut self, handler: Box<dyn UpdateHandler + Send + Sync>) {
        self.handlers.push(handler);
    }
}

impl UpdateHandler for HandlerChain {
    fn log(&self, log: &VeilidLog) {
        for handler in self.handlers.iter() {
            handler.log(log)
        }
    }
    fn app_message(&self, message: &VeilidAppMessage) {
        for handler in self.handlers.iter() {
            handler.app_message(message)
        }
    }
    fn app_call(&self, call: &VeilidAppCall) {
        for handler in self.handlers.iter() {
            handler.app_call(call)
        }
    }
    fn state_attachment(&self, attachment: &VeilidStateAttachment) {
        for handler in self.handlers.iter() {
            handler.state_attachment(attachment)
        }
    }
    fn state_network(&self, network: &VeilidStateNetwork) {
        for handler in self.handlers.iter() {
            handler.state_network(network)
        }
    }
    fn state_config(&self, config: &VeilidStateConfig) {
        for handler in self.handlers.iter() {
            handler.state_config(config)
        }
    }
    fn route_change(&self, change: &VeilidRouteChange) {
        for handler in self.handlers.iter() {
            handler.route_change(change)
        }
    }
    fn value_change(&self, change: &VeilidValueChange) {
        for handler in self.handlers.iter() {
            handler.value_change(change)
        }
    }
    fn shutdown(&self) {
        for handler in self.handlers.iter() {
            handler.shutdown()
        }
    }
}

/// Handle state attachment by distributing changes to a watch channel.
pub struct StateAttachmentWatcher {
    state_attachment_tx: watch::Sender<VeilidStateAttachment>,
}

impl StateAttachmentWatcher {
    /// Return a new handler and a receiver of changes to state attachment.
    pub fn new() -> (Self, watch::Receiver<VeilidStateAttachment>) {
        let (state_attachment_tx, state_attachment_rx) = watch::channel(default_state_attachment());
        (
            Self {
                state_attachment_tx,
            },
            state_attachment_rx,
        )
    }
}

impl UpdateHandler for StateAttachmentWatcher {
    fn state_attachment(&self, attachment: &VeilidStateAttachment) {
        debug!(?attachment);
        let _ = self.state_attachment_tx.send_replace(attachment.clone());
    }
}

fn default_state_attachment() -> VeilidStateAttachment {
    VeilidStateAttachment {
        state: AttachmentState::Detached,
        public_internet_ready: false,
        local_network_ready: false,
        uptime: 0.into(),
        attached_uptime: None,
        reliable_peer_count: 0.into(),
        live_peer_count: 0.into(),
        estimated_network_size: 0.into(),
        median_latency: None,
        over_attached_nodes: 0.into(),
    }
}
