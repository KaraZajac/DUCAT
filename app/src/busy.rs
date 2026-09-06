//! What the app is waiting on, in a sentence.
//!
//! Posting a listing, listing a publication worldwide, forming a group's
//! board, publishing a home: each blocks a screen for five seconds to two
//! minutes, and a button that only says "Posting…" for that long looks
//! stuck. The flows say which phase they are in at each boundary —
//! `say("stamping the notice")`, `say("writing to the board")` — and the
//! screen that started the call polls [`note`] and shows the phrase.
//!
//! One note for the whole process, not one per flow: a person presses one
//! button at a time, and a lap turn that overlaps it borrows the line for
//! a phase, which is a small untruth next to a blank one.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static NOTE: Mutex<Option<String>> = Mutex::new(None);
/// When the note was last set, in milliseconds since the process started.
/// Monotonic, so a clock wound back cannot make a note look fresh.
static SINCE: AtomicU64 = AtomicU64::new(0);
static START: OnceLock<Instant> = OnceLock::new();

/// A note nobody cleared is a phase that died with its thread, not a
/// wait: after this long it is not shown.
const STALE_MS: u64 = 20 * 60 * 1000;

fn now_ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// The phase that is running now, as a short lowercase phrase.
pub fn say(phase: impl Into<String>) {
    *NOTE.lock().unwrap_or_else(|e| e.into_inner()) = Some(phase.into());
    SINCE.store(now_ms(), Ordering::Relaxed);
}

/// The operation is over, however it ended.
pub fn clear() {
    *NOTE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// What is being waited on, if anything.
pub fn note() -> Option<String> {
    let note = NOTE.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
    (now_ms().saturating_sub(SINCE.load(Ordering::Relaxed)) < STALE_MS).then_some(note)
}

/// How long the current phase has been running, in milliseconds.
pub fn since_ms() -> Option<u64> {
    note().map(|_| now_ms().saturating_sub(SINCE.load(Ordering::Relaxed)))
}

/// Clears the note when dropped, so an operation that leaves through `?`
/// does not leave its last phase on the screen.
pub struct Scope(());

pub fn scope() -> Scope {
    Scope(())
}

impl Drop for Scope {
    fn drop(&mut self) {
        clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // One note for the process: the tests take turns.
    static TURN: Mutex<()> = Mutex::new(());

    #[test]
    fn says_and_clears() {
        let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
        say("stamping the notice");
        assert_eq!(note().as_deref(), Some("stamping the notice"));
        assert!(since_ms().is_some());
        say("writing to the board");
        assert_eq!(note().as_deref(), Some("writing to the board"));
        clear();
        assert_eq!(note(), None);
        assert_eq!(since_ms(), None);
    }

    #[test]
    fn scope_clears_on_the_way_out() {
        let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
        fn inner() -> Result<(), ()> {
            let _phase = scope();
            say("forming the board");
            Err(())
        }
        let _ = inner();
        assert_eq!(note(), None);
    }
}
