use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug)]
pub struct ConnectionHandle {
    connection_id: NetworkConnectionId,
    flow: Flow,
    channel: flume::Sender<(Option<Id>, Bytes)>,
    dead: Arc<AtomicBool>,
}

#[derive(Debug)]
pub enum ConnectionHandleSendResult {
    Sent,
    NotSent(Bytes),
}

impl ConnectionHandle {
    pub(super) fn new(
        connection_id: NetworkConnectionId,
        flow: Flow,
        channel: flume::Sender<(Option<Id>, Bytes)>,
        dead: Arc<AtomicBool>,
    ) -> Self {
        Self {
            connection_id,
            flow,
            channel,
            dead,
        }
    }

    #[expect(dead_code)]
    pub fn connection_id(&self) -> NetworkConnectionId {
        self.connection_id
    }

    #[expect(dead_code)]
    pub fn flow(&self) -> Flow {
        self.flow
    }

    pub fn unique_flow(&self) -> UniqueFlow {
        UniqueFlow {
            flow: self.flow,
            connection_id: Some(self.connection_id),
        }
    }

    // #[cfg_attr(feature = "instrument", instrument(level="trace", target="net", skip(self, message), fields(message.len = message.len())))]
    // pub fn send(&self, message: Bytes) -> ConnectionHandleSendResult {
    //     match self.channel.send((Span::current().id(), message)) {
    //         Ok(()) => ConnectionHandleSendResult::Sent,
    //         Err(e) => ConnectionHandleSendResult::NotSent(e.0 .1),
    //     }
    // }

    #[cfg_attr(feature = "instrument", instrument(level="trace", target="net", skip(self, message), fields(message.len = message.len())))]
    pub async fn send_async(&self, message: Bytes) -> ConnectionHandleSendResult {
        // A dead connection's processor no longer drains the channel; queueing would silently drop
        if self.dead.load(Ordering::Relaxed) {
            return ConnectionHandleSendResult::NotSent(message);
        }
        match self
            .channel
            .send_async((Span::current().id(), message))
            .await
        {
            Ok(()) => ConnectionHandleSendResult::Sent,
            Err(e) => ConnectionHandleSendResult::NotSent(e.0 .1),
        }
    }
}

impl PartialEq for ConnectionHandle {
    fn eq(&self, other: &Self) -> bool {
        self.connection_id == other.connection_id && self.flow == other.flow
    }
}

impl Eq for ConnectionHandle {}
