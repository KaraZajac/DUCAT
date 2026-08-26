package org.ducatproject.ducat

import android.content.Context
import org.json.JSONObject

/**
 * Live position after the accept (§15.12) — the state either side of one ride
 * keeps about it.
 *
 * **This is the ladder's last rung and the only one that shares a *now*.** The
 * gate is the accept ceremony: nothing here may run before a `RIDE_ACCEPT`
 * exists in the thread, because the same stream before that is a
 * stranger-tracking primitive, which is the thing §5.2.3 exists to refuse.
 * Watching your driver approach is safe *because* both parties have chosen
 * each other and are about to be physically co-present anyway.
 *
 * Two halves, kept apart on purpose:
 *
 * - **Sending.** A record this device created and owns, one subkey, overwritten
 *   every cadence. Its key and a fresh stream key were sealed into the thread
 *   once as a kind-11 message; the record on the network is noise to anyone
 *   who was not a party.
 * - **Receiving.** The reference the counterparty sent, plus the highest
 *   counter seen — the replay guard, which is stateful and therefore lives
 *   here rather than in the frame decoder.
 *
 * **Consent is per ride, per direction, and off by default.** There is
 * deliberately no standing setting: a toggle that shares on every future ride
 * converts one moment's consent into a policy nobody remembers choosing.
 * Starting is always an act on one ride's screen, and [stop] is called on the
 * receipt, on a retract, or when the ride's record is settled — §15.12's three
 * bounds, enforced by the caller that knows which happened.
 */
object Positions {
    private const val TAG = "Positions"

    /** §15.12's fixed cadence while sharing. A constant heartbeat leaks
     *  liveness and nothing else; an adaptive one turns the update pattern
     *  itself into a channel. */
    const val CADENCE_MS = 4_000L

    /** Past this, a position is rendered as staleness rather than drawn — a
     *  receiver MUST show "last seen 40 s ago", never a guessed position. */
    const val STALE_AFTER_MS = 30_000L

    private fun prefs(context: Context) = securePrefs(context, "ducat_contacts")

    private fun key(personaHex: String) = "pos_$personaHex"

    private fun load(context: Context, personaHex: String): JSONObject =
        prefs(context).getString(key(personaHex), null)
            ?.let { runCatching { JSONObject(it) }.getOrNull() }
            ?: JSONObject()

    private fun save(context: Context, personaHex: String, o: JSONObject) {
        prefs(context).edit().putString(key(personaHex), o.toString()).apply()
        ContactStore.bump()
    }

    // ---- sending ----------------------------------------------------------

    /** True while this device is sharing its position with that contact. */
    fun sharing(context: Context, personaHex: String): Boolean =
        load(context, personaHex).optString("send_record").isNotEmpty()

    /**
     * Begin sharing: mint a record and a key, and hand the reference over.
     *
     * A **new record and a new key every ride** — reuse would make the record
     * key a long-lived identifier linking one ride to the next, which is the
     * linkability the whole design spends effort avoiding elsewhere.
     *
     * The reference goes as an ordinary sealed kind-11 message, so it inherits
     * the thread's encryption and its ordering; the caller is responsible for
     * only calling this after an accept.
     */
    fun start(context: Context, contact: Contact): Boolean {
        if (sharing(context, contact.personaHex)) return true
        val rec = runCatching { uniffi.ducat_mobile.nodeDhtCreate(1u) }.getOrElse {
            DucatLog.w(TAG, "position record: ${it.message}")
            return false
        }
        val streamKey = ByteArray(32).also { java.security.SecureRandom().nextBytes(it) }
        val sent = runCatching {
            Mailbox.send(
                context, contact,
                context.getString(R.string.pos_shared_message),
                PersonaStore(context).personaHex(),
                kind = 11,
                positionRecord = rec.key,
                positionStreamKey = streamKey,
            )
        }
        if (sent.isFailure) {
            DucatLog.w(TAG, "position reference: ${sent.exceptionOrNull()?.message}")
            // The record exists and nobody was told about it: forget it rather
            // than leave an owned record nothing will ever write to (§18.7).
            runCatching { uniffi.ducat_mobile.nodeDhtDelete(rec.key) }
            return false
        }
        val o = load(context, contact.personaHex)
        o.put("send_record", rec.key)
        o.put("send_owner_pub", hexOf(rec.ownerPublic))
        o.put("send_owner_sec", hexOf(rec.ownerSecret))
        o.put("send_key", hexOf(streamKey))
        o.put("send_counter", 0L)
        save(context, contact.personaHex, o)
        DucatLog.i(TAG, "sharing position with ${contact.displayName()}")
        return true
    }

    /**
     * Write one update. Called on the cadence while the ride's screen is open.
     *
     * Returns false when there is nothing to write or the write failed — the
     * caller keeps its loop going either way, because a missed update is a
     * gap the receiver renders as staleness, which is the honest answer.
     */
    fun push(context: Context, personaHex: String, latE7: Long, lonE7: Long, heading: Int?): Boolean {
        val o = load(context, personaHex)
        val record = o.optString("send_record").ifEmpty { return false }
        val streamKey = hexToBytes(o.optString("send_key")) ?: return false
        val next = o.optLong("send_counter") + 1
        val nonce = ByteArray(24).also { java.security.SecureRandom().nextBytes(it) }
        val value = runCatching {
            uniffi.ducat_mobile.positionSeal(
                streamKey, record, nonce,
                uniffi.ducat_mobile.PositionFrameIo(
                    counter = next.toULong(),
                    latE7 = latE7,
                    lonE7 = lonE7,
                    heading = heading?.toUShort(),
                    captured = (System.currentTimeMillis() / 1000).toULong(),
                ),
            )
        }.getOrElse {
            DucatLog.w(TAG, "seal position: ${it.message}")
            return false
        }
        val ok = runCatching {
            // Re-opened as the owner each time: creating a record leaves it
            // writable only for that process, and after a restart a plain set
            // comes back "value is not writable" — the same lesson the outbox
            // learned (see Mailbox.send).
            uniffi.ducat_mobile.nodeDhtOpen(
                record,
                hexToBytes(o.optString("send_owner_pub")) ?: ByteArray(0),
                hexToBytes(o.optString("send_owner_sec")) ?: ByteArray(0),
            )
            uniffi.ducat_mobile.nodeDhtSet(record, 0u, value)
        }.isSuccess
        if (ok) {
            o.put("send_counter", next)
            save(context, personaHex, o)
        }
        return ok
    }

    // ---- receiving --------------------------------------------------------

    /** Remember the reference a counterparty sent (§15.12's kind 11). */
    fun remember(context: Context, personaHex: String, record: String, streamKey: ByteArray) {
        if (record.isBlank() || streamKey.size != 32) return
        val o = load(context, personaHex)
        // A fresh reference supersedes: a new ride mints a new record and key,
        // and the counter restarts with it, so the replay guard resets too.
        if (o.optString("recv_record") != record) {
            o.put("recv_record", record)
            o.put("recv_key", hexOf(streamKey))
            o.put("recv_counter", 0L)
            save(context, personaHex, o)
            DucatLog.i(TAG, "position stream offered by ${personaHex.take(8)}…")
        }
    }

    /** True when the counterparty has offered a stream this device can read. */
    fun watching(context: Context, personaHex: String): Boolean =
        load(context, personaHex).optString("recv_record").isNotEmpty()

    /** One position, as last read. */
    data class Fix(val latE7: Long, val lonE7: Long, val heading: Int?, val capturedSecs: Long)

    /**
     * Read the counterparty's current position, or null.
     *
     * **A non-increasing counter is dropped**, which is §15.12's in-ride replay
     * guard: the record holds one value and an attacker who can rewrite it
     * with an older ciphertext would otherwise move the dot backwards.
     */
    fun pull(context: Context, personaHex: String): Fix? {
        val o = load(context, personaHex)
        val record = o.optString("recv_record").ifEmpty { return null }
        val streamKey = hexToBytes(o.optString("recv_key")) ?: return null
        val raw = runCatching {
            uniffi.ducat_mobile.nodeDhtOpen(record, ByteArray(0), ByteArray(0))
            uniffi.ducat_mobile.nodeDhtGet(record, 0u, true)
        }.getOrNull() ?: return null
        if (raw.isEmpty()) return null
        val frame = runCatching {
            uniffi.ducat_mobile.positionOpen(streamKey, record, raw)
        }.getOrElse {
            // A value that does not authenticate is not this stream's: either
            // the sender stopped and blanked it, or somebody wrote noise. Both
            // are "no position", never a guess.
            return null
        }
        val seen = frame.counter.toLong()
        if (seen <= o.optLong("recv_counter")) {
            DucatLog.w(TAG, "position replay from ${personaHex.take(8)}… — dropped")
            return null
        }
        o.put("recv_counter", seen)
        save(context, personaHex, o)
        return Fix(frame.latE7, frame.lonE7, frame.heading?.toInt(), frame.captured.toLong())
    }

    // ---- stopping ---------------------------------------------------------

    /**
     * End both halves for this contact (§15.12's bound, enforced twice).
     *
     * Sharing MUST stop at the receipt, at a retract naming the stream, or at
     * expiry — whichever is first — and the record's own TTL forgets even if a
     * client does not. At stop the sender blanks the subkey and deletes local
     * record state (§18.7's stewardship, the same rule as a spent hail), and
     * the receiver discards the reference: **the map shows where they are, not
     * where they have been**, and a client that kept a peer's track would have
     * rebuilt pairwise exactly what §5.2.3 refused publicly.
     */
    fun stop(context: Context, personaHex: String) {
        val o = load(context, personaHex)
        o.optString("send_record").takeIf { it.isNotEmpty() }?.let { rec ->
            runCatching {
                uniffi.ducat_mobile.nodeDhtOpen(
                    rec,
                    hexToBytes(o.optString("send_owner_pub")) ?: ByteArray(0),
                    hexToBytes(o.optString("send_owner_sec")) ?: ByteArray(0),
                )
                // Blank it first, so a replica serving the record after we are
                // gone serves nothing rather than our last position.
                uniffi.ducat_mobile.nodeDhtSet(rec, 0u, ByteArray(0))
            }
            runCatching { uniffi.ducat_mobile.nodeDhtDelete(rec) }
        }
        if (o.length() > 0) {
            prefs(context).edit().remove(key(personaHex)).apply()
            ContactStore.bump()
            DucatLog.i(TAG, "position stream ended for ${personaHex.take(8)}…")
        }
    }

    private fun hexOf(b: ByteArray) = b.joinToString("") { "%02x".format(it) }
}
