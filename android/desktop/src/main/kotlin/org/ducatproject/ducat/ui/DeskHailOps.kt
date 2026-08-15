package org.ducatproject.ducat.ui

import android.content.Context
import org.ducatproject.ducat.RideStore

/** See DeskGlue.kt: the desk posts no hails yet, so there is nothing to
 *  sweep — but the poller calls this on every pass, and the name must
 *  exist for Mailbox.kt to compile unchanged. */
fun sweepHailTombstones(context: Context) {
    // Deliberately empty until the desk grows a rider side; RideStore on
    // this platform holds no tombstones to retry.
    if (false) RideStore(context).tombstones()
}
