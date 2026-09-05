use super::*;

impl RPCProcessor {
    pub fn debug_info_nodeinfo(&self) -> String {
        let mut out = String::new();
        let inner = self.inner.lock();
        out += &format!(
            "RPC Worker Dequeue Latency:\n{}",
            indent_all_string(format!("{:#}", inner.rpc_worker_dequeue_latency))
        );
        out += "\n";
        out += &format!(
            "RPC Worker Process Latency:\n{}",
            indent_all_string(format!("{:#}", inner.rpc_worker_process_latency))
        );

        out += "\n";
        let rpc_message_processing_latency_string = inner
            .rpc_worker_process_latency_and_accounting_by_operation_kind
            .iter()
            .map(|(k, (ls, _))| format!("{:>16}: {:#}", k, ls))
            .collect::<Vec<_>>()
            .join("\n");

        out += &format!(
            "RPC Message Processing Latency:\n{}",
            indent_all_string(&rpc_message_processing_latency_string)
        );

        out
    }
}
