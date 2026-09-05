use super::*;

/// Node information exchanged during every RPC message
#[derive(Default, Debug, Clone)]
pub(in crate::rpc_processor) struct SenderPeerInfo {
    /// The current peer info of the sender if required
    opt_peer_info: Option<Arc<PeerInfo>>,
    /// The last timestamp of the target's node info to assist remote node with sending its latest node info
    target_node_info_ts: Timestamp,
}
impl SenderPeerInfo {
    pub fn new_no_peer_info(target_node_info_ts: Timestamp) -> Self {
        Self {
            opt_peer_info: None,
            target_node_info_ts,
        }
    }
    pub fn new(peer_info: Arc<PeerInfo>, target_node_info_ts: Timestamp) -> Self {
        Self {
            opt_peer_info: Some(peer_info),
            target_node_info_ts,
        }
    }
    pub fn opt_peer_info(&self) -> Option<Arc<PeerInfo>> {
        self.opt_peer_info.clone()
    }
    pub fn target_node_info_ts(&self) -> Timestamp {
        self.target_node_info_ts
    }
}
