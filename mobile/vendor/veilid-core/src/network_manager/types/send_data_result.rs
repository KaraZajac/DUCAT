use super::*;

#[derive(Debug)]
pub struct SendDataResult {
    /// Pre-send outcome, including which contact method was picked (or why one wasn't).
    node_contact_method_result: NodeContactMethodResult,
    /// Wire-level outcome; carries the UniqueFlow only on success.
    network_result: NetworkResult<UniqueFlow>,
}

impl SendDataResult {
    pub fn new(
        node_contact_method_result: NodeContactMethodResult,
        network_result: NetworkResult<UniqueFlow>,
    ) -> Self {
        Self {
            node_contact_method_result,
            network_result,
        }
    }

    pub fn direct_dial_info(&self) -> Option<DialInfo> {
        self.node_contact_method_result.direct_dial_info()
    }

    pub fn node_contact_method_result(&self) -> &NodeContactMethodResult {
        &self.node_contact_method_result
    }

    pub fn network_result(&self) -> &NetworkResult<UniqueFlow> {
        &self.network_result
    }

    pub fn into_network_result(self) -> NetworkResult<UniqueFlow> {
        self.network_result
    }

    pub fn destructure(self) -> (NodeContactMethodResult, NetworkResult<UniqueFlow>) {
        (self.node_contact_method_result, self.network_result)
    }

    /// Transport actually used on success, else derived from the attempted contact method.
    pub fn opt_transport_type(&self) -> Option<TransportType> {
        if let NetworkResult::Value(uf) = &self.network_result {
            return Some(uf.flow.transport_type());
        }
        self.node_contact_method_result.opt_transport_type()
    }

    /// Success-path accessor. Returns `None` if `network_result` is not `Value`.
    pub fn unique_flow(&self) -> Option<UniqueFlow> {
        match &self.network_result {
            NetworkResult::Value(uf) => Some(*uf),
            _ => None,
        }
    }

    pub fn transport_type(&self) -> Option<TransportType> {
        self.unique_flow().map(|uf| uf.flow.transport_type())
    }

    pub fn sequence_ordering(&self) -> Option<SequenceOrdering> {
        self.unique_flow()
            .map(|uf| uf.flow.protocol_type().sequence_ordering())
    }
}

impl fmt::Display for SendDataResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.network_result {
            NetworkResult::Value(uf) => write!(f, "flow={}", f.to_string(uf.flow))?,
            other => write!(f, "{}", other)?,
        }
        write!(f, " ncm={}", f.to_string(&self.node_contact_method_result))
    }
}
