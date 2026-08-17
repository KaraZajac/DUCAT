// What the shared logic expects the platform to provide, desk edition.
//
// Notify on a phone posts to the notification shade; the desk's window
// registers a sink here and rings the system tray. The funnel property the
// shared Mailbox promises — if it was stored, it was announced — holds
// either way: an unset sink only means a headless desk (arbiter, smoke)
// stays quiet on purpose. sweepHailTombstones is the rider-side board
// hygiene (ui/Hail.kt); the desk neither posts nor withdraws hails yet, so
// its sweep is empty by construction rather than by neglect — RideStore
// holds no tombstones here.

package org.ducatproject.ducat

import android.content.Context

object Notify {
    /** The window's bell, when a window exists. */
    @Volatile
    var sink: ((from: String, personaHex: String, m: StoredMessage) -> Unit)? = null

    fun message(context: Context, from: String, personaHex: String, m: StoredMessage) {
        DucatLog.i("Desk", "message from $from")
        // Machinery stays quiet: ceremony rounds (8–10) drive the threshold
        // engine and withdrawals (5) un-say something — neither is news the
        // way words and money are.
        if (m.kind == 5 || m.kind in 8..10) return
        sink?.invoke(from, personaHex, m)
    }
}
