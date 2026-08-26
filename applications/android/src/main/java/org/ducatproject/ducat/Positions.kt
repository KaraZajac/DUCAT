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

    /**
     * How long an accept can keep a ride live without any other news.
     *
     * Twelve hours is far longer than a ride and far shorter than a thread.
     * The bound exists for the deals that end without leaving a mark — an
     * unbonded ride nobody sent a receipt for, a hail that was accepted and
     * then simply abandoned — where every other test here has nothing to read.
     */
    const val LIVE_FOR_SECS = 12L * 60 * 60

    /** Allowance between two phones' clocks when ordering an escrow against
     *  an accept the other side stamped. An hour is far more than NTP-backed
     *  handsets drift and far less than the gap between two separate deals. */
    const val SKEW_SECS = 60L * 60

    /** Empty reads before the receiver accepts that the stream is over. */
    const val BLANKS_BEFORE_LETTING_GO = 2

    /**
     * Is there a ride here that a position stream may accompany?
     *
     * **One predicate, because the offer and the bound must not disagree.**
     * Built with the card gated on "an accept exists" and the sweep gated on
     * "the escrow is finished", and the two promptly contradicted each other:
     * tapping Share on a settled ride's thread minted a record, sealed the
     * reference, and the poller wiped it one tick later — the far side saw
     * "offered" and "ended" back to back. Now both ask this.
     *
     * §15.12 gives three bounds and this is all three:
     *
     * - **The accept**, without which the stream is a stranger-tracking
     *   primitive rather than two people who have chosen each other.
     * - **The receipt** — settlement observed, which for a bonded ride is the
     *   escrow reaching a finished stage, and for an unbonded one is a kind-3
     *   receipt landing after the accept.
     * - **Expiry**, which the record's own TTL enforces even against a client
     *   that forgets; nothing here has to.
     *
     * A thread with an old, settled ride in it answers false, which is what
     * makes the card absent there rather than offering something that will be
     * taken away.
     */
    fun rideIsLive(context: Context, personaHex: String): Boolean {
        val thread = ContactStore(context).thread(personaHex)
        val accept = thread.filter { it.kind == 7 }.maxByOrNull { it.timestamp } ?: return false
        // Expiry. A ride is hours; an accept from last week is history, and
        // without this a months-old thread keeps offering to start a stream.
        // The record's TTL bounds a stream already running — it cannot bound
        // an offer to begin a new one, which is what this card is.
        // Seconds on both sides: a Message.timestamp is unix *seconds*
        // (Mailbox writes currentTimeMillis()/1000), and mixing the two here
        // reads every accept as expired — a card that never appears, which
        // looks exactly like this predicate working.
        if (System.currentTimeMillis() / 1000 - accept.timestamp > LIVE_FOR_SECS) return false
        // A receipt after the accept is the ride paid for and over.
        if (thread.any { it.kind == 3 && it.timestamp >= accept.timestamp }) return false
        // And a bonded ride ends when its escrow does, whoever released it.
        //
        // dealWith, not rideWith: the latter hides an escrow whose banner has
        // had its day, and "no banner" is not "no escrow". Asked the wrong way
        // this returned null for a ride settled yesterday and read it as live.
        val ride = Ceremony.dealWith(context, personaHex)
        // …but only a deal from this ride or later can be the thing that ended
        // it. The same two people deal repeatedly: yesterday's settled sale is
        // the newest escrow in the thread right up until today's ride builds
        // one, and reading it as this ride's ending would blank the card on a
        // ride that is actively running.
        //
        // `created` is millis (currentTimeMillis) and a Message.timestamp is
        // seconds; and an *incoming* accept is stamped by the sender's clock,
        // so the two are not even the same clock. SKEW_SECS is the allowance,
        // and it is spent in the safe direction — a slightly-too-old escrow
        // still counts as this ride's, because a card that quietly stops being
        // offered is a smaller wrong than one that offers a stream the poller
        // revokes a tick later.
        val ridesAge = ride != null &&
            ride.optLong("created") / 1000 >= accept.timestamp - SKEW_SECS
        if (ridesAge && Ceremony.isFinished(ride!!)) return false
        return true
    }

    /**
     * §15.12's bound, swept from the poller so it holds with the phone in a
     * pocket — the one bound that must not depend on somebody looking at a
     * screen. A stream whose ride is no longer live is stopped: the record
     * blanked and deleted, the reference and counter discarded.
     */
    fun enforceBounds(context: Context): Int {
        var n = 0
        for (c in ContactStore(context).all()) {
            if (!sharing(context, c.personaHex) && !watching(context, c.personaHex)) continue
            if (rideIsLive(context, c.personaHex)) continue
            stop(context, c.personaHex)
            n += 1
        }
        return n
    }

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
                hexToBytes(o.optString("send_owner_pub")),
                hexToBytes(o.optString("send_owner_sec")),
            )
            uniffi.ducat_mobile.nodeDhtSet(record, 0u, value)
        }.onFailure { note(personaHex, "write position: ${it.message}") }.isSuccess
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
            // **null, not empty.** node_dht_open reads (Some, Some) as "here is
            // a writer keypair" and builds one out of whatever bytes it is
            // given — two empty arrays included. Opening a counterparty's
            // record read-only means *no* writer, and passing ByteArray(0)
            // opened it as a writer with a zero-length key: every read failed,
            // and because every failure in here is a deliberate null, the card
            // sat on "Waiting for their position…" for ever with nothing in
            // the log. The mailbox has always passed null here; this did not.
            uniffi.ducat_mobile.nodeDhtOpen(record, null, null)
            uniffi.ducat_mobile.nodeDhtGet(record, 0u, true)
        }.getOrElse {
            note(personaHex, "read position: ${it.message}")
            return null
        } ?: return null
        if (raw.isEmpty()) {
            // **An empty slot means "ended" only once it has meant something
            // else first.** [start] mints the record and sends the reference
            // before the first frame exists, so the receiver's first reads are
            // legitimately empty and must not be read as a goodbye. After a
            // frame has been seen, empty is the sender's own [stop] blanking
            // the slot, and that is the signal to let go: without it the card
            // went on ageing the last fix for ever — "last seen 40 minutes
            // ago", on a stream whose sender had explicitly ended it — and the
            // reference outlived the sharing it referred to.
            //
            // Two in a row, because one empty read from a replica that has not
            // caught up would otherwise end a live stream.
            if (o.optLong("recv_counter") > 0) {
                val blanks = o.optInt("recv_blanks") + 1
                if (blanks >= BLANKS_BEFORE_LETTING_GO) {
                    forgetReceived(context, personaHex)
                    DucatLog.i(TAG, "position stream closed by ${personaHex.take(8)}…")
                } else {
                    o.put("recv_blanks", blanks)
                    save(context, personaHex, o)
                }
            }
            return null
        }
        if (o.optInt("recv_blanks") != 0) {
            o.put("recv_blanks", 0)
            save(context, personaHex, o)
        }
        val frame = runCatching {
            uniffi.ducat_mobile.positionOpen(streamKey, record, raw)
        }.getOrElse {
            // A value that does not authenticate is not this stream's: either
            // the sender stopped and blanked it, or somebody wrote noise. Both
            // are "no position", never a guess.
            return null
        }
        val seen = frame.counter.toLong()
        val known = o.optLong("recv_counter")
        // **Lower is an attack; equal is just quiet.**
        //
        // The record holds one value that the sender overwrites, so reading it
        // twice between two writes returns the same frame — the normal case
        // whenever the cadence and the read drift apart, or the sender's phone
        // cannot get a fix. Treating that as a replay and returning null threw
        // away a frame that had *authenticated*, and the card fell back to
        // "Waiting for their position…" — reporting no position at all for a
        // stream that was working. That is the one rendering §15.12 forbids in
        // the other direction too: the age is the truth here, and returning
        // the same frame again lets the card age it honestly. Only a counter
        // that goes *backwards* is somebody rewriting the slot with an older
        // ciphertext, and that is still dropped.
        if (seen < known) {
            DucatLog.w(TAG, "position replay from ${personaHex.take(8)}… — dropped")
            return null
        }
        if (seen > known) {
            o.put("recv_counter", seen)
            save(context, personaHex, o)
        }
        return Fix(frame.latE7, frame.lonE7, frame.heading?.toInt(), frame.captured.toLong())
    }

    /**
     * Drop what we hold of *their* stream, leaving ours alone.
     *
     * The two directions are independent (§15.12): a rider may share while the
     * driver does not, so the end of one says nothing about the other.
     */
    private fun forgetReceived(context: Context, personaHex: String) {
        val o = load(context, personaHex)
        o.remove("recv_record"); o.remove("recv_key")
        o.remove("recv_counter"); o.remove("recv_blanks")
        save(context, personaHex, o)
        ContactStore.bump()
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
                // Both halves or neither — see the note in [pull].
                uniffi.ducat_mobile.nodeDhtOpen(
                    rec,
                    hexToBytes(o.optString("send_owner_pub")),
                    hexToBytes(o.optString("send_owner_sec")),
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

    /**
     * Say it once, not eighty times an hour.
     *
     * Both loops run on a four-second cadence, so a failure that logs plainly
     * buries the log inside a minute — which is why the read failure that cost
     * an afternoon was silent in the first place: the choice was between
     * nothing and a flood, and nothing won. Keyed by contact and message, so a
     * *changed* failure still prints and a persistent one prints once.
     */
    private val said = java.util.concurrent.ConcurrentHashMap<String, String>()

    private fun note(personaHex: String, msg: String) {
        if (said.put(personaHex, msg) == msg) return
        DucatLog.w(TAG, "${personaHex.take(8)}…: $msg")
    }

    private fun hexOf(b: ByteArray) = b.joinToString("") { "%02x".format(it) }
}
