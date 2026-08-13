package org.ducatproject.ducat

import android.content.Context
import android.util.Base64
import uniffi.ducat_mobile.OwnedOutput
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import org.json.JSONArray
import org.json.JSONObject

/**
 * Where contacts, cards and message threads live on the device.
 *
 * Deliberately plain: `SharedPreferences` holding JSON. This is **not** the
 * final home for it — §16.10 names a message log as the most sensitive thing
 * the app will hold, and the right storage is an encrypted database keyed by
 * something the OS keystore protects. What is here is honest about being a
 * first pass, and the shape of the API is what a real store would expose so the
 * swap does not reach into the UI.
 *
 * One thing it does get right, because the whole forward-secrecy property rests
 * on it: **a consumed one-time prekey secret is deleted, not marked**. §16.11
 * is only true if the bytes are gone.
 */
class ContactStore(context: Context) {
    private val prefs = context.getSharedPreferences("ducat_contacts", Context.MODE_PRIVATE)

    companion object {
        /**
         * One lock for the whole store, across every instance.
         *
         * Scoping the counters to their own helpers shrank the lost-update
         * window but did not close it: read-modify-write is still three steps,
         * and the responder's coroutine and a chat screen's coroutine both run
         * on the IO dispatcher. A message arriving while a screen wrote back
         * still lost one of the two updates, and the symptom was the *next*
         * inbound message being refused as out of order — a report that points
         * at the message rather than at the write that dropped a counter.
         *
         * The lock is on the companion because callers construct a fresh
         * `ContactStore` per operation; a per-instance lock would guard nothing.
         */
        private val lock = Any()

        /**
         * Bumped by every mutation, so screens can notice one.
         *
         * Without this the chat screen only re-read the store inside its own
         * send handler, so an inbound message was written, decrypted, chained
         * and stored — and then sat there invisible until the user happened to
         * send something. It looked exactly like messages not being delivered,
         * and it was the opposite: everything worked except the redraw.
         *
         * A counter rather than the data itself, because the store is
         * file-backed and the interesting question is only "has anything
         * changed"; the screens re-read what they need.
         */
        private val _changes = MutableStateFlow(0L)
        val changes: StateFlow<Long> = _changes

        internal fun bump() {
            _changes.value = _changes.value + 1
        }
    }

    fun all(): List<Contact> {
        val raw = prefs.getString("contacts", null) ?: return emptyList()
        val arr = JSONArray(raw)
        return (0 until arr.length())
            .map { Contact.from(arr.getJSONObject(it)) }
            // Contacts saved before §16.12 have no outbox and can never send or
            // receive again. Dropping them beats listing people whose messages
            // will silently fail — a broken contact is worse than an absent one,
            // because it looks like it should work.
            .filter { it.myOutbox.isNotEmpty() && it.theirOutbox.isNotEmpty() }
    }

    fun add(c: Contact) { synchronized(lock) {
        val existing = all().filterNot { it.personaHex == c.personaHex }
        save(existing + c)
    } }

    fun update(c: Contact) = add(c)

    /**
     * Advance only the *sending* counters, re-reading first.
     *
     * The chat screen and the responder both used to write the whole record.
     * The screen's copy of a contact is captured when it opens, so sending a
     * message wrote back a stale `inSeq` and silently undid every message
     * received since — after which the next inbound message was refused as out
     * of order, and the sender was told nothing useful. Read-modify-write on a
     * shared record needs the read to happen at write time, not at screen open.
     */
    fun advanceOutbound(personaHex: String, seq: Long, prevLink: ByteArray) { synchronized(lock) {
        val c = all().firstOrNull { it.personaHex == personaHex } ?: return
        save(all().filterNot { it.personaHex == personaHex } +
            c.copy(outSeq = seq, outPrevLink = prevLink))
    } }

    /** The same, for the receiving counters. */
    fun advanceInbound(personaHex: String, seq: Long, prevLink: ByteArray) { synchronized(lock) {
        val c = all().firstOrNull { it.personaHex == personaHex } ?: return
        save(all().filterNot { it.personaHex == personaHex } +
            c.copy(inSeq = seq, inPrevLink = prevLink))
    } }

    /**
     * Drop messages older than the contact's disappearing window.
     *
     * **One-sided, and the UI must say so.** This deletes our copy; it cannot
     * reach theirs, and a design that implied otherwise would be worse than
     * having no feature. What it does give is real: §16.11 already makes a
     * delivered message unrecoverable once its prekey is consumed, so removing
     * the plaintext is the last copy on this device.
     */
    fun expireOld(personaHex: String, afterSecs: Long): Int {
        if (afterSecs <= 0) return 0
        val cutoff = System.currentTimeMillis() / 1000 - afterSecs
        val kept = thread(personaHex).filter { it.timestamp >= cutoff }
        val all = thread(personaHex)
        if (kept.size == all.size) return 0
        writeThread(personaHex, kept)
        return all.size - kept.size
    }

    /** Delete one message from this device. */
    fun deleteMessage(personaHex: String, seq: Long, outgoing: Boolean) {
        writeThread(
            personaHex,
            thread(personaHex).filterNot { it.seq == seq && it.outgoing == outgoing },
        )
    }

    private fun writeThread(personaHex: String, msgs: List<StoredMessage>) = synchronized(lock) {
        val arr = JSONArray()
        msgs.forEach { arr.put(it.toJson()) }
        prefs.edit().putString("thread_$personaHex", arr.toString()).apply()
        bump()
    }

    /** How long messages in this conversation survive locally, in seconds. */
    fun disappearAfter(personaHex: String): Long =
        prefs.getLong("disappear_$personaHex", 0L)

    fun setDisappearAfter(personaHex: String, secs: Long) = synchronized(lock) {
        prefs.edit().putLong("disappear_$personaHex", secs).apply()
        bump()
    }

    /** Show or hide a conversation without touching the contact. */
    fun setChatVisible(personaHex: String, visible: Boolean) { synchronized(lock) {
        val c = all().firstOrNull { it.personaHex == personaHex } ?: return
        save(all().filterNot { it.personaHex == personaHex } + c.copy(chatVisible = visible))
    } }

    /**
     * Delete a conversation's messages.
     *
     * Genuinely deleted, not flagged. §16.11 spends real effort making a
     * delivered message unrecoverable; a "delete" that leaves the plaintext in
     * the store would undo that at the last step, which is the step the user
     * can see.
     *
     * The chain counters reset with it: a thread with no history has no link
     * for the next message to follow, and leaving them would refuse everything
     * the other side sends afterwards.
     */
    fun deleteThread(personaHex: String) { synchronized(lock) {
        prefs.edit().remove("thread_$personaHex").apply()
        bump()
        val c = all().firstOrNull { it.personaHex == personaHex } ?: return
        save(
            all().filterNot { it.personaHex == personaHex } +
                c.copy(outSeq = 0, outPrevLink = null, inSeq = 0, inPrevLink = null)
        )
    } }

    /** Forget a person entirely: the contact and everything they said. */
    fun forget(personaHex: String) { synchronized(lock) {
        prefs.edit().remove("thread_$personaHex").apply()
        save(all().filterNot { it.personaHex == personaHex })
    } }

    /** Record their published keys without touching any counter. */
    fun setTheirBundle(personaHex: String, bundle: ByteArray) { synchronized(lock) {
        val c = all().firstOrNull { it.personaHex == personaHex } ?: return
        save(all().filterNot { it.personaHex == personaHex } + c.copy(theirBundle = bundle))
    } }

    fun remove(personaHex: String) = save(all().filterNot { it.personaHex == personaHex })

    private fun save(list: List<Contact>) { synchronized(lock) {
        val arr = JSONArray()
        list.forEach { arr.put(it.toJson()) }
        prefs.edit().putString("contacts", arr.toString()).apply()
        bump()
    } }

    // --- threads ----------------------------------------------------------

    fun thread(personaHex: String): List<StoredMessage> {
        val raw = prefs.getString("thread_$personaHex", null) ?: return emptyList()
        val arr = JSONArray(raw)
        return (0 until arr.length()).map { StoredMessage.from(arr.getJSONObject(it)) }
    }

    fun append(personaHex: String, m: StoredMessage) { synchronized(lock) {
        val arr = JSONArray()
        (thread(personaHex) + m).forEach { arr.put(it.toJson()) }
        prefs.edit().putString("thread_$personaHex", arr.toString()).apply()
        bump()
    } }

    // --- prekeys ----------------------------------------------------------

    /** The records behind the card we last handed out. */
    fun saveIssuedCard(
        inboxKey: String,
        writerPublic: ByteArray,
        writerSecret: ByteArray,
        outboxKey: String,
        outboxOwnerPublic: ByteArray,
        outboxOwnerSecret: ByteArray,
    ) = synchronized(lock) {
        prefs.edit()
            .putString("issued_inbox", inboxKey)
            .putString("issued_wpub", b64(writerPublic))
            .putString("issued_wsec", b64(writerSecret))
            .putString("issued_outbox", outboxKey)
            .putString("issued_outbox_pub", b64(outboxOwnerPublic))
            .putString("issued_outbox_sec", b64(outboxOwnerSecret))
            .putBoolean("issued_answered", false)
            .apply()
        bump()
    }

    fun issuedCard(): IssuedCardState? {
        val inbox = prefs.getString("issued_inbox", null) ?: return null
        return IssuedCardState(
            inboxKey = inbox,
            writerPublic = unb64(prefs.getString("issued_wpub", "") ?: ""),
            writerSecret = unb64(prefs.getString("issued_wsec", "") ?: ""),
            outboxKey = prefs.getString("issued_outbox", "") ?: "",
            outboxOwnerPublic = unb64(prefs.getString("issued_outbox_pub", "") ?: ""),
            outboxOwnerSecret = unb64(prefs.getString("issued_outbox_sec", "") ?: ""),
        )
    }

    fun issuedCardAnswered(): Boolean = prefs.getBoolean("issued_answered", false)

    fun markIssuedCardAnswered() = synchronized(lock) {
        prefs.edit().putBoolean("issued_answered", true).apply()
        bump()
    }

    /** Our own published bundle and its secrets. */
    fun savePrekeys(bundle: ByteArray, signedSecret: ByteArray, oneTime: Map<Int, ByteArray>) { synchronized(lock) {
        val o = JSONObject()
        o.put("bundle", b64(bundle))
        o.put("signed", b64(signedSecret))
        val ot = JSONObject()
        oneTime.forEach { (id, sk) -> ot.put(id.toString(), b64(sk)) }
        o.put("one_time", ot)
        prefs.edit().putString("prekeys", o.toString()).apply()
    } }

    fun prekeyBundle(): ByteArray? =
        prefs.getString("prekeys", null)?.let { unb64(JSONObject(it).getString("bundle")) }

    fun signedPrekeySecret(): ByteArray? =
        prefs.getString("prekeys", null)?.let { unb64(JSONObject(it).getString("signed")) }

    fun oneTimeSecret(id: Int): ByteArray? {
        val raw = prefs.getString("prekeys", null) ?: return null
        val ot = JSONObject(raw).getJSONObject("one_time")
        return if (ot.has(id.toString())) unb64(ot.getString(id.toString())) else null
    }

    /**
     * Delete a used one-time secret. This is the operation §16.11's forward
     * secrecy consists of — after it, the message that key opened cannot be
     * opened again by anyone, including us.
     */
    fun burnOneTime(id: Int) { synchronized(lock) {
        val raw = prefs.getString("prekeys", null) ?: return
        val o = JSONObject(raw)
        o.getJSONObject("one_time").remove(id.toString())
        // **And prune the published bundle.** Deleting the secret alone leaves
        // the bundle advertising a key that can no longer decrypt anything, and
        // senders take the first one-time entry — so the first key consumed is
        // offered forever and every later message is refused, identically after
        // a re-fetch, because the stale bundle is what gets re-served.
        runCatching {
            uniffi.ducat_mobile.prunePrekey(unb64(o.getString("bundle")), id.toUInt())
        }.onSuccess { o.put("bundle", b64(it)) }
        prefs.edit().putString("prekeys", o.toString()).apply()
    } }

    /**
     * Drop advertised keys we no longer hold a secret for, and report what is
     * left.
     *
     * Pruning on burn stops the corruption spreading; it does not undo what is
     * already written. A store that burned before that fix existed still
     * advertises those ids, senders take the first entry, and so it keeps
     * handing out a key that cannot decrypt — forever, and identically after a
     * re-fetch. Repair has to be explicit, and it has to run on load rather
     * than on write, because the damage predates the code that would prevent it.
     */
    fun reconcilePrekeys(): Int = synchronized(lock) {
        val raw = prefs.getString("prekeys", null) ?: return 0
        val o = JSONObject(raw)
        val secrets = o.getJSONObject("one_time")
        var bundle = runCatching { unb64(o.getString("bundle")) }.getOrNull() ?: return 0
        val advertised = runCatching {
            uniffi.ducat_mobile.bundleOneTimeIds(bundle).map { it.toInt() }
        }.getOrDefault(emptyList())

        var dropped = 0
        for (id in advertised) {
            if (!secrets.has(id.toString())) {
                bundle = runCatching {
                    uniffi.ducat_mobile.prunePrekey(bundle, id.toUInt())
                }.getOrDefault(bundle)
                dropped++
            }
        }
        if (dropped > 0) {
            o.put("bundle", b64(bundle))
            prefs.edit().putString("prekeys", o.toString()).apply()
        }
        return advertised.size - dropped
    }

    /**
     * How many usable one-time keys are left.
     *
     * Counted from the **bundle**, not the secret map, because the bundle is
     * what senders see: a supply that looks healthy locally but advertises
     * nothing usable is the failure this whole method exists to prevent.
     */
    fun oneTimeRemaining(): Int {
        val raw = prefs.getString("prekeys", null) ?: return 0
        val o = JSONObject(raw)
        val advertised = runCatching {
            uniffi.ducat_mobile.bundleOneTimeCount(unb64(o.getString("bundle"))).toInt()
        }.getOrDefault(0)
        return minOf(advertised, o.getJSONObject("one_time").length())
    }
}

data class Contact(
    val personaHex: String,
    val petname: String?,
    val assertedName: String?,
    /** Our append-only log for this contact (§16.12). Only we write it. */
    val myOutbox: String,
    /**
     * The keypair that owns [myOutbox].
     *
     * Creating a record leaves it writable only for that process. Re-opening it
     * without the owner keypair gives a read-only handle, and the write then
     * fails with "value is not writable" — which reads as a permissions problem
     * with the network and is us having thrown the key away.
     */
    val myOutboxOwnerPublic: ByteArray = ByteArray(0),
    val myOutboxOwnerSecret: ByteArray = ByteArray(0),
    /** Theirs. Permanent, and readable whether or not they are online. */
    val theirOutbox: String,
    /** Their published prekeys, read out of the inbox at handshake time. */
    val theirBundle: ByteArray? = null,
    /** Our next outgoing sequence number, and the link it must carry (§16.10). */
    val outSeq: Long = 0,
    val outPrevLink: ByteArray? = null,
    val inSeq: Long = 0,
    val inPrevLink: ByteArray? = null,
    /**
     * Whether this contact appears in the chat list.
     *
     * A contact and a conversation are different things: removing a chat should
     * not throw away the person, and removing the person should not be the only
     * way to tidy the list. Hidden here, deleted in Contacts.
     */
    val chatVisible: Boolean = true,
) {
    /** §7.5: the petname wins. A self-asserted name is a fallback, never a name. */
    fun displayName(): String = petname ?: assertedName ?: "${personaHex.take(8)}…"

    fun toJson(): JSONObject = JSONObject().apply {
        put("persona", personaHex)
        put("petname", petname ?: JSONObject.NULL)
        put("asserted", assertedName ?: JSONObject.NULL)
        put("my_outbox", myOutbox)
        put("my_outbox_pub", b64(myOutboxOwnerPublic))
        put("my_outbox_sec", b64(myOutboxOwnerSecret))
        put("their_outbox", theirOutbox)
        put("their_bundle", theirBundle?.let { b64(it) } ?: JSONObject.NULL)
        put("out_seq", outSeq)
        put("out_prev", outPrevLink?.let { b64(it) } ?: JSONObject.NULL)
        put("in_seq", inSeq)
        put("in_prev", inPrevLink?.let { b64(it) } ?: JSONObject.NULL)
        put("chat_visible", chatVisible)
    }

    companion object {
        fun from(o: JSONObject) = Contact(
            personaHex = o.getString("persona"),
            petname = o.optStringOrNull("petname"),
            assertedName = o.optStringOrNull("asserted"),
            myOutbox = o.optString("my_outbox", ""),
            myOutboxOwnerPublic = unb64(o.optString("my_outbox_pub", "")),
            myOutboxOwnerSecret = unb64(o.optString("my_outbox_sec", "")),
            theirOutbox = o.optString("their_outbox", ""),
            theirBundle = o.optStringOrNull("their_bundle")?.let { unb64(it) },
            outSeq = o.optLong("out_seq"),
            outPrevLink = o.optStringOrNull("out_prev")?.let { unb64(it) },
            inSeq = o.optLong("in_seq"),
            inPrevLink = o.optStringOrNull("in_prev")?.let { unb64(it) },
            chatVisible = o.optBoolean("chat_visible", true),
        )
    }
}

data class StoredMessage(
    val outgoing: Boolean,
    val seq: Long,
    val body: String,
    val timestamp: Long,
    /** 0 text, 1 request, 2 notice (§16.13). */
    val kind: Int = 0,
    val amountPxmr: Long = 0,
    /** Where a request asks to be paid, if it named one. */
    val payto: String? = null,
    /** False means it went out under the signed prekey — no forward secrecy
     *  until that key rotates (§16.11). Shown, not hidden. */
    val forwardSecret: Boolean = true,
    val delivered: Boolean = true,
) {
    fun toJson(): JSONObject = JSONObject().apply {
        put("out", outgoing); put("seq", seq); put("body", body)
        put("ts", timestamp); put("fs", forwardSecret); put("delivered", delivered)
        put("kind", kind); put("amt", amountPxmr)
        put("payto", payto ?: JSONObject.NULL)
    }

    companion object {
        fun from(o: JSONObject) = StoredMessage(
            outgoing = o.getBoolean("out"),
            seq = o.getLong("seq"),
            body = o.getString("body"),
            timestamp = o.getLong("ts"),
            forwardSecret = o.optBoolean("fs", true),
            delivered = o.optBoolean("delivered", true),
            kind = o.optInt("kind", 0),
            amountPxmr = o.optLong("amt", 0L),
            payto = o.optStringOrNull("payto"),
        )
    }
}

/** Our own display name, and the last card we issued. */
class NameStore(context: Context) {
    private val prefs = context.getSharedPreferences("ducat_contacts", Context.MODE_PRIVATE)
    fun get(): String? = prefs.getString("my_name", null)
    fun put(v: String) = prefs.edit().putString("my_name", v).apply()
}

/** The inbox and outbox behind a card we have handed out. */
data class IssuedCardState(
    val inboxKey: String,
    val writerPublic: ByteArray,
    val writerSecret: ByteArray,
    val outboxKey: String,
    val outboxOwnerPublic: ByteArray,
    val outboxOwnerSecret: ByteArray,
)

class CardStore(context: Context) {
    private val prefs = context.getSharedPreferences("ducat_contacts", Context.MODE_PRIVATE)

    fun cardBytes(): ByteArray? = prefs.getString("my_card", null)?.let { unb64(it) }
}

private fun JSONObject.optStringOrNull(k: String): String? =
    if (isNull(k)) null else optString(k, "").ifBlank { null }

private fun b64(b: ByteArray): String = Base64.encodeToString(b, Base64.NO_WRAP)
private fun unb64(s: String): ByteArray = Base64.decode(s, Base64.NO_WRAP)

/**
 * The persona key this device signs contact cards with.
 *
 * Created once, lazily, and kept. §4.1 puts persona keys in software precisely
 * so they can be backed up — a hardware-bound persona is a persona that dies
 * with the phone, taking every contact and every attestation with it.
 *
 * Stored here in plain `SharedPreferences`, which is the same first-pass
 * compromise as the rest of this file and is **not** where it should end up:
 * §4.3's backup format exists for this key, and the on-device copy belongs
 * behind the OS keystore.
 */
class PersonaStore(context: Context) {
    private val prefs = context.getSharedPreferences("ducat_contacts", Context.MODE_PRIVATE)

    /** Our own persona, in the same hex form contacts are keyed by. */
    fun personaHex(): String =
        uniffi.ducat_mobile.personaPublicHex(secret())

    fun secret(): ByteArray {
        prefs.getString("persona_secret", null)?.let { return unb64(it) }
        val fresh = uniffi.ducat_mobile.createPersonaSecret()
        prefs.edit().putString("persona_secret", b64(fresh)).apply()
        return fresh
    }
}

/**
 * The AAD binding a ciphertext to one conversation (§16.11).
 *
 * **Must be symmetric.** The first version used "the other party's persona",
 * which reads correctly on each side and is a different value on each side:
 * A sealing to B used B's key, and B opening from A used A's. Nothing ever
 * decrypted. Sorting the pair gives both ends the same bytes without either
 * needing to know which of them started the conversation.
 */
fun threadAad(minePersonaHex: String, theirsPersonaHex: String): ByteArray =
    listOf(minePersonaHex, theirsPersonaHex).sorted().joinToString(":").toByteArray()


/**
 * The Monero wallet created during onboarding.
 *
 * It was previously held only in onboarding's Compose state, so the address a
 * user was shown during setup vanished the moment setup finished — and
 * `BackupSettings` was being handed `null` for the key it exists to back up.
 * A wallet you cannot see the address of is a wallet nobody can pay into.
 *
 * The spend key lives here for §4.3's export. That is the same first-pass
 * compromise as the rest of this file, and the loudest instance of it: this is
 * the key that controls the money.
 */
class WalletStore(context: Context) {
    private val prefs = context.getSharedPreferences("ducat_contacts", Context.MODE_PRIVATE)

    fun save(address: String, spendKeyHex: String, restoreHeight: ULong, stagenet: Boolean) {
        prefs.edit()
            .putString("wallet_address", address)
            .putString("wallet_spend", spendKeyHex)
            .putString("wallet_height", restoreHeight.toString())
            .putBoolean("wallet_stagenet", stagenet)
            .apply()
    }

    fun address(): String? = prefs.getString("wallet_address", null)

    // --- scan state -------------------------------------------------------

    fun scannedTo(): Long = prefs.getLong("wallet_scanned_to", 0L)
    fun tip(): Long = prefs.getLong("wallet_tip", 0L)

    /**
     * Record a window's progress and anything it found.
     *
     * Outputs are keyed by key image so a rescan cannot double-count. A wallet
     * that adds the same output twice reports a balance it does not have, and
     * the mistake compounds every scan.
     */
    /**
     * Blocks per second, measured rather than assumed.
     *
     * Scanning speed depends on the node, the link and how full the blocks are,
     * so a constant would be wrong on most devices most of the time. An
     * estimate built from a guess is worse than no estimate: people plan around
     * the number they are shown.
     */
    fun scanRate(): Double = prefs.getFloat("wallet_rate", 0f).toDouble()

    /**
     * Why the last scan attempt failed, if it did.
     *
     * Kept because the alternative is a screen that says "not started" while
     * the reason sits in logcat, which a person holding a phone cannot read.
     * A wallet that will not sync has to be able to say what stopped it.
     */
    fun lastScanError(): String? = prefs.getString("wallet_scan_error", null)

    fun recordScanError(msg: String?) = prefs.edit()
        .putString("wallet_scan_error", msg)
        .apply()
        .also { ContactStore.bump() }

    fun recordScan(scannedTo: Long, tip: Long, found: List<OwnedOutput>) {
        val now = System.currentTimeMillis()
        val lastAt = prefs.getLong("wallet_scan_at", 0L)
        val lastTo = prefs.getLong("wallet_scanned_to", 0L)
        if (lastAt > 0 && scannedTo > lastTo) {
            val secs = (now - lastAt) / 1000.0
            if (secs > 0.5) {
                val observed = (scannedTo - lastTo) / secs
                // Smoothed: one slow window on a bad connection should nudge the
                // estimate, not replace it and make the remaining time jump.
                val prev = prefs.getFloat("wallet_rate", 0f).toDouble()
                val blended = if (prev > 0) prev * 0.7 + observed * 0.3 else observed
                prefs.edit().putFloat("wallet_rate", blended.toFloat()).apply()
            }
        }
        prefs.edit().putLong("wallet_scan_at", now).apply()
        val byKi = entries().associateBy { it.keyImage }.toMutableMap()
        for (o in found) {
            val ki = o.keyImageHex
            if (ki.isEmpty()) continue
            byKi[ki] = WalletEntry(
                amountPxmr = o.amountPxmr.toLong(),
                height = o.height.toLong(),
                spent = byKi[ki]?.spent ?: false,
                keyImage = ki,
                blob = o.blob,
            )
        }
        writeEntries(byKi.values.toList())
        prefs.edit()
            .putLong("wallet_scanned_to", scannedTo)
            .putLong("wallet_tip", tip)
            .apply()
    }

    fun recordSpent(status: Map<String, Boolean>) {
        writeEntries(entries().map { it.copy(spent = status[it.keyImage] ?: it.spent) })
    }

    fun entries(): List<WalletEntry> {
        val raw = prefs.getString("wallet_outputs", null) ?: return emptyList()
        val arr = JSONArray(raw)
        return (0 until arr.length()).map {
            val o = arr.getJSONObject(it)
            WalletEntry(
                amountPxmr = o.getLong("amt"),
                height = o.getLong("h"),
                spent = o.optBoolean("spent", false),
                keyImage = o.getString("ki"),
                blob = Base64.decode(o.optString("blob", ""), Base64.NO_WRAP),
            )
        }
    }

    private fun writeEntries(list: List<WalletEntry>) {
        val arr = JSONArray()
        list.forEach {
            arr.put(JSONObject().apply {
                put("amt", it.amountPxmr); put("h", it.height)
                put("spent", it.spent); put("ki", it.keyImage)
                put("blob", Base64.encodeToString(it.blob, Base64.NO_WRAP))
            })
        }
        prefs.edit().putString("wallet_outputs", arr.toString()).apply()
    }
    fun spendKeyHex(): String? = prefs.getString("wallet_spend", null)
    fun stagenet(): Boolean = prefs.getBoolean("wallet_stagenet", true)
    fun restoreHeight(): ULong =
        prefs.getString("wallet_height", null)?.toULongOrNull() ?: 0uL

    /**
     * Move the scan back to a height and forget what was found after it.
     *
     * Needed because a wallet created before the app could reach a node has a
     * restore height of zero, and scanning from genesis at a few hundred blocks
     * a step is thirty hours of crawling to reach the present. That is
     * indistinguishable, from the screen, from a wallet with no money.
     *
     * Outputs are cleared rather than kept: a rescan that starts before them
     * would find them again, and they are keyed by key image so nothing would
     * double — but leaving stale entries from a range about to be re-read makes
     * "what has this scan actually seen" unanswerable.
     */
    fun rescanFrom(height: Long) {
        prefs.edit()
            .putString("wallet_height", height.toString())
            .putLong("wallet_scanned_to", height)
            .remove("wallet_outputs")
            // The measured rate and its timestamp belong to the range being
            // abandoned. Keeping them means the next window is timed against a
            // clock reading from before the skip — possibly hours — which
            // collapses the rate and shows an estimate of days for a scan about
            // to finish in a minute.
            .remove("wallet_rate")
            .remove("wallet_scan_at")
            .remove("wallet_scan_error")
            .apply()
        // Same change signal the contact store uses, so every screen watching it
        // re-reads rather than showing the balance it had a moment ago.
        ContactStore.bump()
    }
}

/**
 * The exchange rate, cached.
 *
 * Cached hard on purpose. A price lookup tells whoever answers that this device
 * cares about Monero's price, at a time, from an IP — a smaller disclosure than
 * the wallet already makes to a public node, but one the user did not ask for.
 * Half an hour is a long time for a price and a short time for a pattern.
 */
class RateStore(context: Context) {
    private val prefs = context.getSharedPreferences("ducat_contacts", Context.MODE_PRIVATE)

    companion object {
        /**
         * Currencies the price sources quote directly.
         *
         * Listed rather than passed through, because an unrecognised code comes
         * back from CoinGecko as an absent field rather than an error — which
         * would show as "no price" with nothing saying why.
         */
        val SUPPORTED = listOf(
            "USD", "EUR", "GBP", "CAD", "AUD", "NZD", "CHF", "JPY", "CNY",
            "INR", "BRL", "MXN", "ZAR", "SEK", "NOK", "DKK", "PLN", "TRY",
            "RUB", "KRW", "SGD", "HKD", "TWD", "THB", "IDR", "PHP", "NGN",
            "ARS", "CLP", "CZK", "HUF", "ILS", "AED", "SAR", "UAH", "VND",
        )
    }

    /**
     * Whether amounts lead with the user's currency instead of XMR.
     *
     * A preference rather than a per-screen choice: the unit someone reads a
     * balance in has to be the unit they confirm a payment in, or the check
     * they think they are doing is not the one they are doing.
     */
    fun preferFiat(): Boolean = prefs.getBoolean("rate_prefer_fiat", false)

    fun setPreferFiat(v: Boolean) = prefs.edit().putBoolean("rate_prefer_fiat", v).apply()
        .also { ContactStore.bump() }

    /** Off means off: no request is made at all, not a hidden one. */
    fun enabled(): Boolean = prefs.getBoolean("rate_enabled", true)
    fun setEnabled(v: Boolean) = prefs.edit().putBoolean("rate_enabled", v).apply()

    /**
     * The currency to price in, defaulting to the phone's own.
     *
     * Taken from the device locale rather than assumed to be dollars. Someone
     * in Berlin does not want to convert from USD in their head to know whether
     * a payment was the right size, and defaulting to the currency they already
     * think in is free.
     */
    fun currency(): String = prefs.getString("rate_currency", null) ?: deviceCurrency()

    /** What this phone is set to, or USD when it names something unsupported. */
    fun deviceCurrency(): String = runCatching {
        val code = java.util.Currency
            .getInstance(java.util.Locale.getDefault())
            .currencyCode
            .uppercase()
        if (code in SUPPORTED) code else "USD"
    }.getOrDefault("USD")
    fun setCurrency(v: String) =
        prefs.edit().putString("rate_currency", v).remove("rate_value").apply()

    fun cached(): Pair<Double, Long>? {
        val v = prefs.getFloat("rate_value", 0f).toDouble()
        val at = prefs.getLong("rate_at", 0L)
        return if (v > 0 && at > 0) v to at else null
    }

    fun store(v: Double, at: Long, source: String) = prefs.edit()
        .putFloat("rate_value", v.toFloat())
        .putLong("rate_at", at)
        .putString("rate_source", source)
        .apply()

    fun source(): String = prefs.getString("rate_source", "") ?: ""

    fun isStale(maxAgeSecs: Long = 1800): Boolean {
        val at = cached()?.second ?: return true
        return System.currentTimeMillis() / 1000 - at > maxAgeSecs
    }
}

/** Which Monero node to use, and the last one that worked. */
class NodeStore(context: Context) {
    private val prefs = context.getSharedPreferences("ducat_contacts", Context.MODE_PRIVATE)

    fun ownUrl(): String? = prefs.getString("monero_own_node", null)?.ifBlank { null }

    fun setOwnUrl(v: String?) =
        prefs.edit().putString("monero_own_node", v?.trim() ?: "").apply()

    /**
     * The last node that answered, synced, on the right network.
     *
     * Kept so a restart does not re-probe the whole list before showing
     * anything — not as a preference. A node that was good an hour ago is still
     * checked before it is used.
     */
    fun rememberLastGood(url: String) =
        prefs.edit().putString("monero_last_good", url).apply()

    fun lastGood(): String? = prefs.getString("monero_last_good", null)
}
