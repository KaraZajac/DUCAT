package org.ducatproject.ducat

import kotlinx.coroutines.flow.MutableStateFlow

/**
 * "The network says something you were watching changed."
 *
 * Veilid will push a notification to a watcher, and the bridge surfaces that
 * as one process-wide flag — which `node_wait_change` **consumes**: whoever
 * wakes first clears it, and everybody else's wake-up is gone. So exactly one
 * thing in this process may call it (the poller, which has always owned it),
 * and anything else that wants to know listens here.
 *
 * Its own file because the sweep that listens is shared with the desktop
 * client and the poller that rings is not: a file the desk does not compile
 * cannot be referenced by one it does.
 */
object NetworkRings {
    /**
     * Bumped on every ring. The value is a timestamp only so a listener can
     * tell one ring from the next — nothing reads it as a time.
     */
    val changed = MutableStateFlow(0L)

    /**
     * The record keys behind the most recent rings, waiting to be claimed.
     *
     * A ring alone says "one of the things you watch moved", which leaves a
     * driver watching eighteen boards to read all eighteen to find out which
     * — a lap, for a fare sitting on one of them. The keys turn that into one
     * read. Drained rather than observed, because they are events: whoever
     * takes them has them, and nobody should act on the same change twice.
     */
    private val keys = HashSet<String>()

    /** Record keys from a ring. */
    fun note(changedKeys: Collection<String>) {
        synchronized(keys) { keys += changedKeys }
        changed.value = System.currentTimeMillis()
    }

    /** Take what has changed since the last taker. */
    fun drain(): Set<String> = synchronized(keys) {
        val taken = keys.toSet()
        keys.clear()
        taken
    }
}
