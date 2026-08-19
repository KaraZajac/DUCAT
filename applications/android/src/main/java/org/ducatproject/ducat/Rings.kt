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
}
