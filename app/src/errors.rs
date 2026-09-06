//! An error as a person reads it.
//!
//! What the node and the swarm return is written for a log: `swarm:
//! Failed("TryAgain: allocated route failed to test")`. A screen shows a
//! sentence instead, chosen by the shape of the failure — the network is
//! away, no route yet, the board is full — and never the Rust wrapping.
//! Sentences a flow wrote for the person ([`Error::Refused`]) pass through
//! as they are.

use crate::{CardProblem, Error};

/// The sentence a screen shows for `e`.
pub fn plain(e: &Error) -> String {
    match e {
        Error::Card(CardProblem::AlreadyUsed) => "That card has already been answered. Ask them for a fresh one.".into(),
        Error::Card(CardProblem::Own) => "That card is your own.".into(),
        Error::Card(CardProblem::NotPublished) => "The card's details have not reached the network yet. Try again in a minute.".into(),
        Error::Card(CardProblem::Expired) => "That card has expired. Ask them for a fresh one.".into(),
        Error::Io(io) => format!("Could not read or write a file: {}.", io_words(io)),
        Error::Store(_) => "One of this desk's own records could not be read.".into(),
        // Written for the person already; only a full board, which some
        // flows report in the log's words, is reworded.
        Error::Refused(s) => if board_full(s) { FULL.into() } else { s.clone() },
        Error::Node(s) => by_shape(s).unwrap_or_else(|| format!("The network could not do that: {}.", inner(s))),
        Error::Swarm(s) => by_shape(s).unwrap_or_else(|| {
            // A fetch that failed is the ordinary swarm failure — the seller
            // is away — and it used to get the seeding sentence, so a
            // listing that would not open said "could not be put on the
            // network" to the person trying to read it. Seeding words its
            // own failures with "seed", "announce" or the digest check.
            let l = s.to_lowercase();
            if l.contains("seed") || l.contains("announce") || l.contains("digest") || l.contains("index") {
                "The pictures could not be put on the network. They will be retried.".into()
            } else {
                "The pictures could not be fetched right now; the seller may be away.".into()
            }
        }),
    }
}

const FULL: &str = "Every slot on that board is taken this week.";

fn board_full(s: &str) -> bool {
    let l = s.to_lowercase();
    ["every shard", "is full", "no free slot", "no slot"].iter().any(|n| l.contains(n))
}

/// The common shapes, by their text: what veilid and the node say when
/// the transport is down, when no route has been allocated yet, when a
/// record is not there to read, and what the boards say when full.
fn by_shape(s: &str) -> Option<String> {
    let l = s.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| l.contains(n));
    // The node's own variants arrive in their Debug form: "NotRunning".
    if has(&["not running", "notrunning", "not attached", "notattached", "detached", "no connection", "offline", "shutdown"]) {
        return Some("The network is away right now; try again in a moment.".into());
    }
    if has(&["tryagain", "route"]) {
        return Some("The network has not given this desk a route yet. Give it a minute and try again.".into());
    }
    if board_full(s) {
        return Some(FULL.into());
    }
    if has(&["key not found", "not open", "no such record", "record not found"]) {
        return Some("That record is not reachable right now.".into());
    }
    if has(&["timeout", "timed out"]) {
        return Some("The network did not answer in time; try again in a moment.".into());
    }
    None
}

/// The text inside a `Failed("…")` wrapper, unescaped; the text itself
/// when there is no wrapper. Trailing punctuation is dropped so the
/// sentence around it can end it.
fn inner(s: &str) -> String {
    let mut t = s.trim();
    for prefix in ["Failed(\"", "Failed("] {
        if let Some(rest) = t.strip_prefix(prefix) {
            t = rest.trim_end_matches(')').trim_end_matches('"');
            break;
        }
    }
    t.replace("\\\"", "\"").trim_end_matches(['.', ' ']).to_string()
}

/// An io error's own words, without the "(os error N)" tail and without
/// a path: the message names what happened, the log names where.
fn io_words(e: &std::io::Error) -> String {
    let text = e.to_string();
    let text = match text.find(" (os error") {
        Some(i) => &text[..i],
        None => &text,
    };
    let mut chars = text.trim().chars();
    match chars.next() {
        Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
        None => "an unknown problem".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_route_failure_is_a_route_sentence() {
        let e = Error::Swarm("Failed(\"TryAgain: allocated route failed to test\")".into());
        assert_eq!(plain(&e), "The network has not given this desk a route yet. Give it a minute and try again.");
        let e = Error::Node("Failed(\"route: TryAgain: allocated route failed to test (4 attempts)\")".into());
        assert_eq!(plain(&e), "The network has not given this desk a route yet. Give it a minute and try again.");
    }

    #[test]
    fn a_node_that_is_not_running_is_away() {
        let e = Error::from(ducat_mobile::node::NodeError::NotRunning);
        assert_eq!(plain(&e), "The network is away right now; try again in a moment.");
        let e = Error::Swarm("Failed(\"the node is not running\")".into());
        assert_eq!(plain(&e), "The network is away right now; try again in a moment.");
    }

    #[test]
    fn a_missing_record_and_a_full_board() {
        let e = Error::Node("Failed(\"get: Key not found: VLD0:abc\")".into());
        assert_eq!(plain(&e), "That record is not reachable right now.");
        let e = Error::Node("Failed(\"set: record not open\")".into());
        assert_eq!(plain(&e), "That record is not reachable right now.");
        let e = Error::Refused("every shard of local:dqche is full".into());
        assert_eq!(plain(&e), "Every slot on that board is taken this week.");
    }

    #[test]
    fn io_says_what_without_where() {
        let e = Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory (os error 2)"));
        assert_eq!(plain(&e), "Could not read or write a file: no such file or directory.");
    }

    #[test]
    fn the_fallback_never_shows_the_wrapper() {
        let e = Error::Node("Failed(\"set: subkey 3 refused twice — the network holds a newer value\")".into());
        let s = plain(&e);
        assert!(!s.contains("Failed("), "{s}");
        assert!(!s.contains("node:"), "{s}");
        assert_eq!(s, "The network could not do that: set: subkey 3 refused twice — the network holds a newer value.");
        let e = Error::Swarm("Failed(\"the swarm went quiet\")".into());
        assert_eq!(plain(&e), "The pictures could not be fetched right now; the seller may be away.");
        let e = Error::Swarm("Failed(\"digest is 64 hex chars\")".into());
        assert_eq!(plain(&e), "The pictures could not be put on the network. They will be retried.");
        let e = Error::Swarm("Failed(\"giving up — watchdog fired after 3 event(s)\")".into());
        assert_eq!(plain(&e), "The pictures could not be fetched right now; the seller may be away.");
        let e = Error::Swarm("Failed(\"seed: no such directory\")".into());
        assert_eq!(plain(&e), "The pictures could not be put on the network. They will be retried.");
    }

    #[test]
    fn a_refusal_written_for_the_person_passes_through() {
        let e = Error::Refused("this listing has no area yet".into());
        assert_eq!(plain(&e), "this listing has no area yet");
        let e = Error::Refused("that page reaches the network — https://cdn.example/router.js".into());
        assert_eq!(plain(&e), "that page reaches the network — https://cdn.example/router.js");
        assert_eq!(plain(&Error::Card(CardProblem::Own)), "That card is your own.");
    }
}
