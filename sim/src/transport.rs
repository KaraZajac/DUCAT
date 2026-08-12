//! In-process message bus standing in for a Veilid private route.
//!
//! Messages cross as bytes, exactly as they would on the wire, so signing and
//! canonical-form checks are exercised for real. What is *not* exercised is
//! route establishment, latency, fragmentation, or delivery failure — Phase 0b
//! measured those separately, and conflating them with protocol debugging would
//! make both harder.
//!
//! The queue is deliberately a queue rather than a direct call: a client that
//! only works when the counterparty responds synchronously has assumed
//! something the network does not provide.

use std::collections::VecDeque;

pub struct Wire {
    queue: VecDeque<Vec<u8>>,
    pub log: Vec<String>,
    pub verbose: bool,
    pub bytes_sent: usize,
    pub messages: usize,
}

impl Wire {
    pub fn new(verbose: bool) -> Self {
        Wire {
            queue: VecDeque::new(),
            log: Vec::new(),
            verbose,
            bytes_sent: 0,
            messages: 0,
        }
    }

    pub fn send(&mut self, from: &str, to: &str, kind: &str, bytes: &[u8]) {
        self.bytes_sent += bytes.len();
        self.messages += 1;
        let line = format!("    {:>12} → {:<12} {:<12} {:>5} B", from, to, kind, bytes.len());
        if self.verbose {
            println!("{}", line);
        }
        self.log.push(line);
        self.queue.push_back(bytes.to_vec());
    }

    pub fn recv(&mut self) -> Vec<u8> {
        self.queue.pop_front().unwrap_or_default()
    }

    /// Record something that is not a protocol message — a settlement, a note.
    pub fn note(&mut self, from: &str, to: &str, what: &str) {
        let line = if from.is_empty() {
            format!("    {}", what)
        } else {
            format!("    {:>12} → {:<12} {}", from, to, what)
        };
        if self.verbose {
            println!("{}", line);
        }
        self.log.push(line);
    }
}
