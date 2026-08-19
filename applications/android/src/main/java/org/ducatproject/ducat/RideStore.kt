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
        /** A second copy on the containing 5-cell, when the corner was
         *  deserted (§15.12's density rule). Same card; claim-once referees. */
        val board2: String? = null,
        val subkey2: UInt = 0u,
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
            .putString("board2", r.board2)
            .putInt("subkey2", r.subkey2.toInt())
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
            board2 = prefs.getString("board2", null),
            subkey2 = prefs.getInt("subkey2", 0).toUInt(),
        )
    }

    fun clear() = prefs.edit()
        .remove("board").remove("subkey").remove("inbox")
        .remove("card").remove("expiry").remove("notice")
        .remove("board2").remove("subkey2")
        .apply()

    // --- tombstones -------------------------------------------------------
    //
    // A take-down that runs while the phone is offline fails silently, and
    // the board keeps advertising a withdrawn hail for the next driver to
    // claim (observed live, 2026-08-15: the desk claimed a ghost). A clear
    // is therefore recorded before it is attempted, retried by the poller,
    // and dropped only when the slot is verifiably not ours — or the notice
    // has expired, after which the board self-heals: sweeps filter expired
    // notices and writers treat their slots as free.

    data class Tombstone(val board: String, val subkey: UInt, val card: String, val expiry: Long)

    /**
     * Guards the tombstone list, which posting and sweeping both rewrite
     * whole — a post adds two at once (the fine cell and its wide copy) while
     * a sweep is removing the ones that expired. A lost tombstone is a board
     * slot nobody reclaims.
     */
    private companion object { val lock = Any() }

    fun addTombstone(t: Tombstone) = synchronized(lock) {
        val arr = org.json.JSONArray(prefs.getString("tombstones", "[]"))
        arr.put(org.json.JSONObject()
            .put("b", t.board).put("s", t.subkey.toInt())
            .put("c", t.card).put("e", t.expiry))
        prefs.edit().putString("tombstones", arr.toString()).apply()
    }

    fun tombstones(): List<Tombstone> {
        val arr = org.json.JSONArray(prefs.getString("tombstones", "[]"))
        return (0 until arr.length()).map {
            val o = arr.getJSONObject(it)
            Tombstone(o.getString("b"), o.getInt("s").toUInt(), o.getString("c"), o.getLong("e"))
        }
    }

    fun removeTombstone(t: Tombstone) = synchronized(lock) {
        val keep = tombstones().filterNot { it == t }
        val arr = org.json.JSONArray()
        keep.forEach {
            arr.put(org.json.JSONObject()
                .put("b", it.board).put("s", it.subkey.toInt())
                .put("c", it.card).put("e", it.expiry))
        }
        prefs.edit().putString("tombstones", arr.toString()).apply()
    }
}
