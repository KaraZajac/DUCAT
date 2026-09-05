//! Notices for the person: a message came, money came, somebody is
//! calling. The app queues them; the window drains the queue and shows
//! them however the platform shows things. Nothing here depends on a
//! window existing — a headless run just logs them.

use std::sync::Mutex;

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Notice {
    pub title: String,
    pub body: String,
    /// Which thread to open when the notice is clicked, if any.
    pub open_thread: Option<String>,
    pub at_ms: u64,
}

static QUEUE: Mutex<Vec<Notice>> = Mutex::new(Vec::new());
const KEEP: usize = 50;

pub fn post(title: impl Into<String>, body: impl Into<String>, open_thread: Option<String>) {
    let n = Notice { title: title.into(), body: body.into(), open_thread, at_ms: crate::contacts::now_ms() };
    crate::log::info("Notify", format!("{} — {}", n.title, n.body));
    let mut q = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    q.push(n);
    if q.len() > KEEP {
        let drop = q.len() - KEEP;
        q.drain(0..drop);
    }
    crate::contacts::bump();
}

/// Everything posted since the last take.
pub fn take() -> Vec<Notice> {
    std::mem::take(&mut *QUEUE.lock().unwrap_or_else(|e| e.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_queue_and_drain_once() {
        take();
        post("A", "b", None);
        post("C", "d", Some("hex".into()));
        let got = take();
        assert_eq!(got.len(), 2);
        assert_eq!(got[1].open_thread.as_deref(), Some("hex"));
        assert!(take().is_empty());
    }
}
