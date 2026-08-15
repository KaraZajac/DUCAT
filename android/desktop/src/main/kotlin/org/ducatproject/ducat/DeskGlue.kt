// What the shared logic expects the platform to provide, desk edition.
//
// Notify on a phone posts to the notification shade; on the desk the window
// is its own shade, so arrivals just reach the log until the UI grows a
// bell. sweepHailTombstones is the rider-side board hygiene (ui/Hail.kt);
// the desk neither posts nor withdraws hails yet, so its sweep is empty by
// construction rather than by neglect — RideStore holds no tombstones here.

package org.ducatproject.ducat

import android.content.Context

object Notify {
    fun message(context: Context, from: String, personaHex: String, m: StoredMessage) {
        DucatLog.i("Desk", "message from $from")
    }
}
