// DUCAT modification (see ../../STIGMERGE-NOTICE.md): a route observer.
//
// The seeder answers block requests over private routes this crate
// allocates. On a borrowed node (veilnet's `from_api`) the HOST owns the
// update callback and must demultiplex inbound `AppCall`s: calls arriving
// on swarm routes belong to the seeder, everything else belongs to the
// host's own protocol — and answering the wrong one steals the other's
// single reply slot. The host can only route what it can see, so the
// announcer reports every route it creates or retires here.
//
// A no-op unless an observer is installed, which is exactly the upstream
// behaviour. A local delta we carry (research/post-1.0/upstream/).

use std::sync::RwLock;

use veilid_core::RouteId;

type Observer = Box<dyn Fn(&RouteId, bool) + Send + Sync>;

static OBSERVER: RwLock<Option<Observer>> = RwLock::new(None);

/// Install the observer. Called once by the embedding application; `true`
/// means the route was created, `false` that it was retired.
pub fn set_observer(f: Observer) {
    *OBSERVER.write().unwrap() = Some(f);
}

pub(crate) fn added(route_id: &RouteId) {
    if let Some(f) = OBSERVER.read().unwrap().as_ref() {
        f(route_id, true);
    }
}

pub(crate) fn removed(route_id: &RouteId) {
    if let Some(f) = OBSERVER.read().unwrap().as_ref() {
        f(route_id, false);
    }
}
