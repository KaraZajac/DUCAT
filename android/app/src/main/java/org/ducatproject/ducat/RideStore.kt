package org.ducatproject.ducat

import android.content.Context

/**
 * The one hail this phone has standing (§15.12).
 *
 * A posted notice lives on the DHT, not in a composable: process death or a
 * trip away from Home must not forget which slot the notice occupies or which
 * inbox the claim answers to — the board would keep advertising a ride whose
 * rider can no longer hear the driver. One record, not a list, because a
 * rider hails one ride at a time.
 */
class RideStore(context: Context) {
    private val prefs = context.getSharedPreferences("ducat_rides", Context.MODE_PRIVATE)

    data class PostedRide(
        /** The board shard the notice is pinned to, e.g. "geo:abcdef-2". */
        val board: String,
        val subkey: UInt,
        /** Where a driver's claim answers. */
        val inboxKey: String,
        /** Our card's URI — proof of tenancy when clearing the slot. */
        val cardUri: String,
        /** Epoch seconds; past this the notice is dead either way. */
        val expiry: Long,
        /** The encoded notice itself, so migration reposts the same bytes. */
        val notice: ByteArray = ByteArray(0),
    )

    fun save(r: PostedRide) {
        prefs.edit()
            .putString("board", r.board)
            .putInt("subkey", r.subkey.toInt())
            .putString("inbox", r.inboxKey)
            .putString("card", r.cardUri)
            .putLong("expiry", r.expiry)
            .putString("notice", android.util.Base64.encodeToString(
                r.notice, android.util.Base64.NO_WRAP))
            .apply()
    }

    fun load(): PostedRide? {
        return PostedRide(
            board = prefs.getString("board", null) ?: return null,
            subkey = prefs.getInt("subkey", 0).toUInt(),
            inboxKey = prefs.getString("inbox", null) ?: return null,
            cardUri = prefs.getString("card", null) ?: return null,
            expiry = prefs.getLong("expiry", 0L),
            notice = prefs.getString("notice", null)
                ?.let { android.util.Base64.decode(it, android.util.Base64.NO_WRAP) }
                ?: ByteArray(0),
        )
    }

    fun clear() = prefs.edit().clear().apply()
}
