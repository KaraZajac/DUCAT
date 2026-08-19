package org.ducatproject.ducat

import android.content.Context
import android.content.SharedPreferences
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
    private val prefs = securePrefs(context, "ducat_contacts")

    /** Kept so that forgetting a person can forget what they asked about. */
    private val appContext = context.applicationContext

    companion object {
        /**
         * How long a burned one-time secret stays readable (§16.11).
         *
         * Long enough to cover DHT head propagation — observed lagging a
         * republish by close to a minute — with a wide margin; short enough
         * that the forward-secrecy delete is a promise about tonight, not
         * someday.
         */
        const val BURN_GRACE_MS = 30L * 60 * 1000

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
    /** Clamp a thread's ring to its record's real size (legacy-log healing). */
    fun setMyRing(personaHex: String, ring: Int) { synchronized(lock) {
        val c = all().firstOrNull { it.personaHex == personaHex } ?: return
        save(all().filterNot { it.personaHex == personaHex } + c.copy(myRing = ring))
    } }

    fun advanceInbound(personaHex: String, seq: Long, prevLink: ByteArray?) { synchronized(lock) {
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
     * The chain counters and prev-links stay exactly where they are. They are
     * cursors into a live DHT conversation, not part of the rendering:
     * resetting them forks both directions at once — our next send reuses a
     * sequence their reader already accepted, and their next message is
     * refused as out of order. Deleting a thread removes what this device
     * shows, never where the protocol stands.
     */
    fun deleteThread(personaHex: String) { synchronized(lock) {
        prefs.edit().remove("thread_$personaHex").apply()
        bump()
    } }

    /**
     * Forget a person entirely: the contact, everything they said, and every
     * per-persona key the store and the mailbox filed under them. The prefixed
     * families ("stuck_", "slotseen_") are swept by scanning all keys, because
     * they are keyed per-slot and per-seq and nothing else remembers which
     * ones exist — leaving them would grow the prefs file by one orphan per
     * forgotten conversation, forever.
     */
    fun forget(personaHex: String) { synchronized(lock) {
        val e = prefs.edit()
        listOf("thread_", "disappear_", "seen_", "usedtheirs_", "billseen_", "pendingslot_")
            .forEach { e.remove(it + personaHex) }
        prefs.all.keys.filter {
            it.startsWith("stuck_$personaHex:") || it.startsWith("slotseen_$personaHex:")
        }.forEach { e.remove(it) }
        // The thread's prekey offer dies with the thread; its unconsumed ids
        // are never reassigned (the id counter only climbs), so the secrets
        // simply expire out of use.
        all().firstOrNull { it.personaHex == personaHex }?.let {
            e.remove("prekeys_ob_${it.myOutbox}")
        }
        putContacts(e, all().filterNot { it.personaHex == personaHex })
        e.apply()
        // The listing they enquired about is part of the conversation, and
        // "forget this person" has to mean all of it.
        runCatching { Enquiries.forget(appContext, personaHex) }
        bump()
    } }

    /** Take a fresher address for a contact, from details or a request. */
    fun setTheirAddress(personaHex: String, address: String?) {
        if (address.isNullOrBlank()) return
        val c = all().firstOrNull { it.personaHex == personaHex } ?: return
        if (c.theirAddress == address) return
        save(all().filterNot { it.personaHex == personaHex } + c.copy(theirAddress = address))
    }

    /** Whether we publish our own address so contacts can pay without asking. */
    fun publishAddress(): Boolean = prefs.getBoolean("publish_address", false)

    /**
     * Whether this device publishes read watermarks (§16.16). **Off by
     * default**: when a message was read is behavioural data, and it leaves
     * the device by choice, not by installing a chat app.
     */
    fun readReceipts(): Boolean = prefs.getBoolean("read_receipts", false)
    fun setReadReceipts(v: Boolean) {
        prefs.edit().putBoolean("read_receipts", v).apply(); bump()
    }

    /**
     * The last inbound sequence this user has *seen* — locally, for the
     * unread dot and the tab badge. Not §16.16's watermark: this never leaves
     * the device, so it needs no opt-in.
     */
    fun chatSeen(personaHex: String): Long = prefs.getLong("seen_$personaHex", 0L)

    fun setChatSeen(personaHex: String, v: Long) {
        if (v <= chatSeen(personaHex)) return
        prefs.edit().putLong("seen_$personaHex", v).apply()
        bump()
    }

    /**
     * When a backup was last exported, and whether the things it protects
     * have changed since (§4.3). Contacts and prekeys are the churn that
     * matters: money keys never change, but every new relationship is one a
     * stale bundle will not restore.
     */
    fun markBackupExported() {
        prefs.edit().putLong("backup_at", System.currentTimeMillis())
            .putInt("backup_contacts", all().size).apply()
        bump()
    }

    /** When a backup was last exported, or 0 if one never has been. */
    fun backupExportedAt(): Long = prefs.getLong("backup_at", 0L)

    fun backupStale(): Boolean {
        val at = prefs.getLong("backup_at", 0L)
        if (at == 0L) return all().isNotEmpty()
        return all().size > prefs.getInt("backup_contacts", 0)
    }

    /** Conversations holding messages this user has not looked at. */
    fun unreadThreads(): Int = all().count { it.chatVisible && it.inSeq > chatSeen(it.personaHex) }

    fun setTheirReadUpTo(personaHex: String, v: Long) { synchronized(lock) {
        val c = all().firstOrNull { it.personaHex == personaHex } ?: return
        if (c.theirReadUpTo == v) return
        save(all().filterNot { it.personaHex == personaHex } + c.copy(theirReadUpTo = v))
    } }

    fun setPublishAddress(v: Boolean) = prefs.edit().putBoolean("publish_address", v).apply()
        .also { ContactStore.bump() }

    /** Record their published keys without touching any counter. */
    fun setTheirBundle(personaHex: String, bundle: ByteArray) { synchronized(lock) {
        val c = all().firstOrNull { it.personaHex == personaHex } ?: return
        // A refreshed head replaces the cache — but re-pruned of every id this
        // side ever sealed to. In a two-party thread, their burned set is
        // exactly our spent set, so however stale the fetched head, a spent
        // key cannot be picked twice (the desk relearned this with a coffee
        // receipt that died on the same dead key twice).
        var b = bundle
        usedTheirIds(personaHex).forEach { id ->
            runCatching { uniffi.ducat_mobile.prunePrekey(b, id.toUInt()) }
                .onSuccess { b = it }
        }
        save(all().filterNot { it.personaHex == personaHex } + c.copy(theirBundle = b))
    } }

    fun usedTheirIds(personaHex: String): Set<Int> =
        prefs.getString("usedtheirs_$personaHex", null)
            ?.split(',')?.mapNotNull { it.toIntOrNull() }?.toSet() ?: emptySet()

    fun recordUsedTheirId(personaHex: String, id: Int) { synchronized(lock) {
        val all = usedTheirIds(personaHex) + id
        prefs.edit().putString("usedtheirs_$personaHex", all.joinToString(",")).apply()
    } }

    fun remove(personaHex: String) = save(all().filterNot { it.personaHex == personaHex })

    private fun save(list: List<Contact>) { synchronized(lock) {
        val e = prefs.edit()
        putContacts(e, list)
        e.apply()
        bump()
    } }

    /** The contacts array into a caller's editor, for writes that must land
     *  in the same commit as something else. */
    private fun putContacts(e: SharedPreferences.Editor, list: List<Contact>) {
        val arr = JSONArray()
        list.forEach { arr.put(it.toJson()) }
        e.putString("contacts", arr.toString())
    }

    // --- threads ----------------------------------------------------------

    fun thread(personaHex: String): List<StoredMessage> {
        val raw = prefs.getString("thread_$personaHex", null) ?: return emptyList()
        val arr = JSONArray(raw)
        return (0 until arr.length()).map { StoredMessage.from(arr.getJSONObject(it)) }
    }

    fun append(personaHex: String, m: StoredMessage) { synchronized(lock) {
        val arr = JSONArray()
        (thread(personaHex) + m).forEach { arr.put(it.toJson()) }
        val e = prefs.edit().putString("thread_$personaHex", arr.toString())
        // A receipt is a record, not a message that happens to mention money.
        // Conversations get deleted — a taxi's thread especially — and the
        // receipt must outlive the small talk around it, the way a paper one
        // outlives the ride. Captured here, the one funnel every message
        // passes through, into a store nothing but the user clears.
        if (m.kind == 3) saveReceiptLocked(personaHex, m, e)
        e.apply()
        bump()
    } }

    /**
     * One inbound message and the cursor that accepts it, as one commit.
     *
     * Append and advance used to be two writes, and a process death between
     * them re-delivered the message on the next poll: a duplicate thread row,
     * and a receipt captured twice. The cursor is the statement that this
     * message was taken; it cannot be separable from the message itself.
     */
    fun appendAndAdvance(
        personaHex: String,
        m: StoredMessage,
        newInSeq: Long,
        newPrevLink: ByteArray?,
    ) { synchronized(lock) {
        val e = prefs.edit()
        val arr = JSONArray()
        (thread(personaHex) + m).forEach { arr.put(it.toJson()) }
        e.putString("thread_$personaHex", arr.toString())
        if (m.kind == 3) saveReceiptLocked(personaHex, m, e)
        all().firstOrNull { it.personaHex == personaHex }?.let { c ->
            putContacts(e, all().filterNot { it.personaHex == personaHex } +
                c.copy(inSeq = newInSeq, inPrevLink = newPrevLink))
        }
        e.apply()
        bump()
    } }

    /**
     * The outbound twin: the local echo, the sending counters, and the sealed
     * slot bytes still owed to the DHT, in one commit — before any network
     * write. The failure orders are not symmetric: a published slot and head
     * with the counter lost to a process death would reuse this sequence with
     * different content next time, a fork every reader keeps; a persisted
     * counter with the slot unwritten is only a late slot, which the pending
     * bytes fill in on a later send with the same seq and the same content.
     */
    fun appendAndAdvanceOutbound(
        personaHex: String,
        m: StoredMessage,
        newOutSeq: Long,
        newPrevLink: ByteArray,
        sealedSlot: ByteArray,
    ) { synchronized(lock) {
        val e = prefs.edit()
        val arr = JSONArray()
        (thread(personaHex) + m).forEach { arr.put(it.toJson()) }
        e.putString("thread_$personaHex", arr.toString())
        if (m.kind == 3) saveReceiptLocked(personaHex, m, e)
        e.putString(
            "pendingslot_$personaHex",
            JSONObject().put("seq", m.seq).put("b", b64(sealedSlot)).toString(),
        )
        all().firstOrNull { it.personaHex == personaHex }?.let { c ->
            putContacts(e, all().filterNot { it.personaHex == personaHex } +
                c.copy(outSeq = newOutSeq, outPrevLink = newPrevLink))
        }
        e.apply()
        bump()
    } }

    /** Sealed bytes a send persisted but never delivered: seq to bytes. */
    fun pendingSlot(personaHex: String): Pair<Long, ByteArray>? =
        prefs.getString("pendingslot_$personaHex", null)?.let {
            runCatching {
                val o = JSONObject(it)
                o.getLong("seq") to unb64(o.getString("b"))
            }.getOrNull()
        }

    fun clearPendingSlot(personaHex: String) { synchronized(lock) {
        prefs.edit().remove("pendingslot_$personaHex").apply()
    } }

    /** A receipt, kept apart from the conversation it arrived in. */
    data class ReceiptRecord(
        val txidHex: String?,
        val amountPxmr: Long,
        val items: List<BillItem>,
        val taxPxmr: Long?,
        /** The counterparty's persona, and their name as it read at the time —
         *  kept as text because the contact itself may be deleted later. */
        val contactHex: String,
        val counterparty: String,
        /** True when this device issued it (we were the payee). */
        val mine: Boolean,
        val timestamp: Long,
        /** Settled outside DUCAT: txid-less by construction — no chain event
         *  exists for it, and the ledger must not go looking for one. */
        val oob: Boolean = false,
    )

    private fun saveReceiptLocked(
        personaHex: String,
        m: StoredMessage,
        into: SharedPreferences.Editor? = null,
    ) {
        val name = all().firstOrNull { it.personaHex == personaHex }?.displayName()
            ?: "${personaHex.take(8)}…"
        val arr = prefs.getString("receipts_v1", null)
            ?.let { runCatching { JSONArray(it) }.getOrNull() } ?: JSONArray()
        // Captured once. The same receipt can reach here twice — a poll
        // re-reading a slot, the migration re-walking a thread — and a record
        // store must not count a payment twice because delivery stuttered.
        for (i in 0 until arr.length()) {
            val o = arr.optJSONObject(i) ?: continue
            val sameTx = m.txidHex != null && !o.isNull("txid") &&
                o.optString("txid").equals(m.txidHex, ignoreCase = true)
            val sameMsg = o.optString("hex") == personaHex &&
                o.optLong("seq", -1L) == m.seq && o.optBoolean("mine") == m.outgoing
            if (sameTx || sameMsg) return
        }
        arr.put(JSONObject().apply {
            put("txid", m.txidHex ?: JSONObject.NULL)
            put("amt", m.amountPxmr)
            put("items", JSONArray().also { a ->
                m.items.forEach { a.put(JSONObject().put("d", it.description).put("a", it.amountPxmr)) }
            })
            put("tax", m.taxPxmr ?: JSONObject.NULL)
            put("hex", personaHex)
            put("who", name)
            put("mine", m.outgoing)
            put("ts", m.timestamp)
            put("seq", m.seq)
            if (m.oob) put("oob", true)
        })
        val e = into ?: prefs.edit()
        e.putString("receipts_v1", arr.toString())
        if (into == null) e.apply()
    }

    fun receipts(): List<ReceiptRecord> {
        val raw = prefs.getString("receipts_v1", null) ?: return emptyList()
        val arr = runCatching { JSONArray(raw) }.getOrElse { return emptyList() }
        return (0 until arr.length()).mapNotNull { i ->
            runCatching {
                val o = arr.getJSONObject(i)
                ReceiptRecord(
                    txidHex = if (o.isNull("txid")) null else o.optString("txid"),
                    amountPxmr = o.getLong("amt"),
                    items = (o.optJSONArray("items") ?: JSONArray()).let { a ->
                        (0 until a.length()).map {
                            val it2 = a.getJSONObject(it)
                            BillItem(it2.getString("d"), it2.getLong("a"))
                        }
                    },
                    taxPxmr = if (o.isNull("tax")) null else o.getLong("tax"),
                    contactHex = o.getString("hex"),
                    counterparty = o.optString("who"),
                    mine = o.optBoolean("mine"),
                    timestamp = o.optLong("ts"),
                    oob = o.optBoolean("oob", false),
                )
            }.getOrNull()
        }
    }

    /**
     * One-time import of receipts already sitting in threads, from before the
     * store existed. The damage this repairs is silent: a deleted taxi thread
     * would have taken its receipts with it.
     */
    fun migrateReceipts() { synchronized(lock) {
        if (prefs.getBoolean("receipts_migrated_v1", false)) return
        for (c in all()) {
            for (m in thread(c.personaHex)) {
                if (m.kind == 3) saveReceiptLocked(c.personaHex, m)
            }
        }
        prefs.edit().putBoolean("receipts_migrated_v1", true).apply()
    } }

    // --- backup (§4.3) ----------------------------------------------------

    /**
     * Everything a backup needs to restore the relationships.
     *
     * Typed contacts and prekeys, because another client must be able to take
     * them; the opaque blob carries same-client continuity — threads, tabs,
     * conversation settings, their profiles — with no interop promise. The
     * wallet keys are deliberately absent: the backup already carries them in
     * their own typed fields, and a second copy is a second thing to audit.
     */
    fun backupContacts(): List<uniffi.ducat_mobile.ContactBackup> = all().map { c ->
        uniffi.ducat_mobile.ContactBackup(
            persona = hexToBytes(c.personaHex) ?: ByteArray(0),
            myOutboxKey = c.myOutbox,
            myOutboxOwnerPublic = c.myOutboxOwnerPublic,
            myOutboxOwnerSecret = c.myOutboxOwnerSecret,
            theirOutboxKey = c.theirOutbox,
            theirBundle = c.theirBundle,
            theirPayto = c.theirAddress,
            petname = c.petname,
            assertedName = c.assertedName,
            inSeq = c.inSeq.toULong(),
            outSeq = c.outSeq.toULong(),
            inPrev = c.inPrevLink,
            outPrev = c.outPrevLink,
        )
    }

    fun backupPrekeys(): Triple<ByteArray?, List<uniffi.ducat_mobile.PrekeyEntry>, Long> {
        val raw = prefs.getString("prekeys", null)
            ?: return Triple(null, emptyList(), prefs.getInt("prekey_next_id", 1).toLong())
        val o = JSONObject(raw)
        val ot = o.optJSONObject("one_time") ?: JSONObject()
        val entries = ot.keys().asSequence().map { id ->
            uniffi.ducat_mobile.PrekeyEntry(id.toULong(), unb64(ot.getString(id)))
        }.toList()
        return Triple(
            if (o.has("signed")) unb64(o.getString("signed")) else null,
            entries,
            prefs.getInt("prekey_next_id", 1).toLong(),
        )
    }

    /** The keys that are presentation rather than protocol. */
    // receipts_v1 rides along deliberately: a receipt is the record that
    // must survive everything else — thread deletions, contact deletions,
    // and now device loss too.
    private val appStateKeys = listOf("tabs_v1", "publish_address", "receipts_v1", "claimed_kis_v1")

    fun backupAppState(): ByteArray {
        val o = JSONObject()
        // Threads and per-thread settings, by prefix; the fixed keys after.
        val threads = JSONObject()
        prefs.all.keys.filter {
            it.startsWith("thread_") || it.startsWith("disappear_")
        }.forEach { k -> prefs.getString(k, null)?.let { threads.put(k, it) }
            ?: threads.put(k, prefs.getLong(k, 0L)) }
        o.put("kv", threads)
        appStateKeys.forEach { k ->
            when (val v = prefs.all[k]) {
                null -> {}
                // claimed_kis_v1 is a StringSet; org.json would mangle it and
                // restore silently dropped it — so a restored device forgot
                // which outputs were already matched to a bill, and a still-open
                // tab for the same amount could re-claim a spent payment (the
                // exact double-match claimedKis exists to prevent).
                is Set<*> -> o.put(k, JSONArray(v.toList()))
                else -> o.put(k, v)
            }
        }
        // Their profiles ride inside the contacts JSON already; carry it whole
        // so avatars and pronouns survive on the same client.
        prefs.getString("contacts", null)?.let { o.put("contacts_raw", it) }
        return o.toString().toByteArray(Charsets.UTF_8)
    }

    /**
     * Restore, opaque first and typed second: the typed fields are the
     * authoritative overlay, so a bundle from a different client — no opaque
     * blob, or one this client cannot read — still restores every relationship.
     */
    fun restoreFromBackup(r: uniffi.ducat_mobile.RestoredBackup) = synchronized(lock) {
        r.appState?.let { blob ->
            runCatching {
                val o = JSONObject(String(blob, Charsets.UTF_8))
                val e = prefs.edit()
                o.optJSONObject("kv")?.let { kv ->
                    kv.keys().forEach { k ->
                        val v = kv.get(k)
                        if (v is String) e.putString(k, v) else if (v is Long) e.putLong(k, v)
                        else if (v is Int) e.putLong(k, v.toLong())
                    }
                }
                o.optString("contacts_raw").takeIf { it.isNotEmpty() }
                    ?.let { e.putString("contacts", it) }
                appStateKeys.forEach { k ->
                    if (o.has(k)) when (val v = o.get(k)) {
                        is Boolean -> e.putBoolean(k, v)
                        is String -> e.putString(k, v)
                        is JSONArray -> e.putStringSet(
                            k, (0 until v.length()).map { v.getString(it) }.toSet(),
                        )
                    }
                }
                e.apply()
            }.onFailure { DucatLog.w("Backup", "app state: ${it.message}") }
        }

        // Typed contacts overlay whatever the blob brought.
        for (c in r.contacts) {
            val personaHex = c.persona.toHexString()
            val existing = all().firstOrNull { it.personaHex == personaHex }
            add(
                (existing ?: Contact(
                    personaHex = personaHex,
                    petname = null, assertedName = null,
                    myOutbox = "", theirOutbox = "",
                )).copy(
                    petname = c.petname ?: existing?.petname,
                    assertedName = c.assertedName ?: existing?.assertedName,
                    myOutbox = c.myOutboxKey,
                    myOutboxOwnerPublic = c.myOutboxOwnerPublic,
                    myOutboxOwnerSecret = c.myOutboxOwnerSecret,
                    theirOutbox = c.theirOutboxKey,
                    theirBundle = c.theirBundle,
                    theirAddress = c.theirPayto,
                    inSeq = c.inSeq.toLong(),
                    outSeq = c.outSeq.toLong(),
                    inPrevLink = c.inPrev,
                    outPrevLink = c.outPrev,
                )
            )
        }

        // Prekeys merge in — never replace; the store's one rule.
        val ot = r.prekeyOneTime.associate { it.id.toInt() to it.secret }
        if (r.prekeySignedSecret != null || ot.isNotEmpty()) {
            val bundle = prefs.getString("prekeys", null)
                ?.let { runCatching { unb64(JSONObject(it).getString("bundle")) }.getOrNull() }
                ?: ByteArray(0)
            savePrekeys(bundle, r.prekeySignedSecret ?: ByteArray(0), ot)
        }
        val next = prefs.getInt("prekey_next_id", 1)
        if (r.prekeyNextId.toInt() > next) {
            prefs.edit().putInt("prekey_next_id", r.prekeyNextId.toInt()).apply()
        }
        bump()
    }

    // --- prekeys ----------------------------------------------------------

    /**
     * Every card we have handed out and not yet seen answered.
     *
     * A *registry*, because the single slot this replaced was a live bug twice
     * over. Issuing a card overwrote the previous card's keys, so a code still
     * on somebody's screen — the profile QR, a till mid-sale — died the moment
     * any other card was made, and its claimant connected into silence. And
     * every flow that showed a card watched for "any new contact", so a
     * profile-code scan during a sale would have been billed as the customer.
     * A claim is an answer *to a specific card*, and the registry is what makes
     * that sentence expressible.
     */
    fun saveIssuedCard(
        inboxKey: String,
        writerPublic: ByteArray,
        writerSecret: ByteArray,
        outboxKey: String,
        outboxOwnerPublic: ByteArray,
        outboxOwnerSecret: ByteArray,
        uri: String,
        purpose: String,
        /** Seconds the network copy lives, so pruning can follow it. */
        validSecs: Long = 0,
    ) = synchronized(lock) {
        val arr = prefs.getString("issued_cards", null)?.let { JSONArray(it) } ?: JSONArray()
        arr.put(JSONObject().apply {
            put("inbox", inboxKey); put("wpub", b64(writerPublic)); put("wsec", b64(writerSecret))
            put("outbox", outboxKey); put("opub", b64(outboxOwnerPublic)); put("osec", b64(outboxOwnerSecret))
            put("uri", uri); put("purpose", purpose)
            put("made", System.currentTimeMillis()); put("ttl", validSecs)
            put("answered_by", JSONObject.NULL)
        })
        prefs.edit().putString("issued_cards", arr.toString()).apply()
        bump()
    }

    fun issuedCards(): List<IssuedCardState> {
        val arr = prefs.getString("issued_cards", null)?.let { JSONArray(it) } ?: return emptyList()
        return (0 until arr.length()).map { i ->
            val o = arr.getJSONObject(i)
            IssuedCardState(
                inboxKey = o.getString("inbox"),
                writerPublic = unb64(o.getString("wpub")),
                writerSecret = unb64(o.getString("wsec")),
                outboxKey = o.getString("outbox"),
                outboxOwnerPublic = unb64(o.getString("opub")),
                outboxOwnerSecret = unb64(o.getString("osec")),
                uri = o.optString("uri", ""),
                purpose = o.optString("purpose", "profile"),
                answeredBy = if (o.isNull("answered_by")) null else o.optString("answered_by"),
            )
        }
    }

    fun markCardAnswered(inboxKey: String, personaHex: String) = synchronized(lock) {
        val arr = prefs.getString("issued_cards", null)?.let { JSONArray(it) } ?: return
        for (i in 0 until arr.length()) {
            val o = arr.getJSONObject(i)
            if (o.getString("inbox") == inboxKey) o.put("answered_by", personaHex)
        }
        prefs.edit().putString("issued_cards", arr.toString()).apply()
        bump()
    }

    /** Who answered a given card, if anyone has. */
    fun claimantOf(inboxKey: String): String? =
        issuedCards().firstOrNull { it.inboxKey == inboxKey }?.answeredBy

    /**
     * Sweep the registry (§18.7's stewardship): answered cards a while after
     * their claim was collected, unanswered ones past their day. Returns the
     * inbox keys of what was dropped so the caller can forget the records too
     * — the network reclaims its copies by TTL either way; this is about not
     * being a long-lived origin for spent purposes, and not growing a registry
     * forever.
     */
    fun pruneCards(): List<String> = synchronized(lock) {
        val arr = prefs.getString("issued_cards", null)?.let { JSONArray(it) } ?: return emptyList()
        val now = System.currentTimeMillis()
        val keep = JSONArray()
        val dropped = mutableListOf<String>()
        for (i in 0 until arr.length()) {
            val o = arr.getJSONObject(i)
            val made = o.optLong("made", now)
            val answered = !o.isNull("answered_by")
            // Held exactly as long as the network holds it. A card outlives
            // its usefulness the moment its published copy expires — nobody
            // can claim it after that — and the poller re-arms a watch on
            // every unanswered card still in this registry, on every pass. A
            // flat day meant a counter taking two hundred orders and losing
            // thirty of them mid-pair watched thirty dead records for the
            // next twenty-two hours.
            //
            // The TTL varies by what the card is for: two hours at a kiosk or
            // a till, twelve at a bar or in a taxi, a day for a standing
            // profile code. So it is read from the card rather than guessed
            // from its purpose. An hour of grace covers a clock that drifted;
            // cards written before this recorded a TTL fall back to the day
            // they always had.
            val ttlSecs = o.optLong("ttl", 0L)
            val unansweredLife =
                if (ttlSecs > 0) ttlSecs * 1000L + 60 * 60 * 1000L
                else 24 * 60 * 60 * 1000L
            val stale =
                (answered && now - made > 60 * 60 * 1000L) ||
                    (!answered && now - made > unansweredLife)
            if (stale) dropped += o.getString("inbox") else keep.put(o)
        }
        if (dropped.isNotEmpty()) {
            prefs.edit().putString("issued_cards", keep.toString()).apply()
            bump()
        }
        dropped
    }

    /**
     * The URI of the card currently on offer, so it can be shown without being
     * regenerated.
     *
     * Kept because publishing a card creates two DHT records: making a new one
     * every time somebody opens the code screen would litter the network and
     * hand out a different code each glance.
     */
    fun currentCardUri(): String? =
        issuedCards().lastOrNull { it.purpose == "profile" && it.answeredBy == null }
            ?.uri?.takeIf { it.isNotEmpty() }

    /** Our own published bundle and its secrets. */
    /**
     * Merge new prekey material in. **Never replace.**
     *
     * This used to overwrite the whole record — one-time secrets *and* the
     * signed secret — every time a card was issued or the supply topped up.
     * Peers hold cached copies of old bundles and seal to the keys in them, so
     * every overwrite turned messages already in flight into BadSig, including
     * the signed-prekey fallback that exists precisely for "my other keys are
     * gone". Secrets leave this store one way: [burnOneTime], §16.11's delete.
     */
    fun savePrekeys(bundle: ByteArray, signedSecret: ByteArray, oneTime: Map<Int, ByteArray>) { synchronized(lock) {
        val o = prefs.getString("prekeys", null)?.let { JSONObject(it) } ?: JSONObject()
        // Empty material never overwrites real material: restore passes what
        // it has, and "nothing" must mean "keep", not "erase".
        if (bundle.isNotEmpty()) o.put("bundle", b64(bundle))
        if (!o.has("signed") && signedSecret.size == 32) o.put("signed", b64(signedSecret))
        val ot = o.optJSONObject("one_time") ?: JSONObject()
        oneTime.forEach { (id, sk) -> ot.put(id.toString(), b64(sk)) }
        o.put("one_time", ot)
        prefs.edit().putString("prekeys", o.toString()).apply()
    } }

    /**
     * Ids for the next batch of one-time keys, globally unique on this device.
     *
     * Every batch used to start at 1, so a second card reused ids whose secrets
     * the first card's peer still expected. An id is a name; two keys must
     * never share one.
     */
    fun nextPrekeyStart(count: Int): Int = synchronized(lock) {
        val next = prefs.getInt("prekey_next_id", 1)
        prefs.edit().putInt("prekey_next_id", next + count).apply()
        next
    }

    fun prekeyBundle(): ByteArray? =
        prefs.getString("prekeys", null)?.let { unb64(JSONObject(it).getString("bundle")) }

    // --- per-thread prekey offers -----------------------------------------
    //
    // §16.11: a one-time id is offered to at most one counterparty. One
    // global bundle in every head meant two contacts holding the same cached
    // copy sealed to the same key — the first message in burned it and the
    // second arrived permanently unreadable. Each thread's head now offers a
    // disjoint batch; the secrets stay in the one global map (an id is
    // globally unique on this device), only the *offering* is partitioned.

    /** The bundle this thread's head advertises, if one has been cut for it. */
    fun threadBundle(outbox: String): ByteArray? =
        prefs.getString("prekeys_ob_$outbox", null)?.let { unb64(it) }

    fun setThreadBundle(outbox: String, blob: ByteArray) =
        prefs.edit().putString("prekeys_ob_$outbox", b64(blob)).apply()

    /** How many of a thread's offered one-time ids still hold secrets. */
    fun threadOneTimeRemaining(outbox: String): Int {
        val blob = threadBundle(outbox) ?: return 0
        val secrets = prefs.getString("prekeys", null)
            ?.let { JSONObject(it).getJSONObject("one_time") } ?: return 0
        return runCatching {
            uniffi.ducat_mobile.bundleOneTimeIds(blob).count { secrets.has(it.toString()) }
        }.getOrDefault(0)
    }

    fun signedPrekeySecret(): ByteArray? =
        prefs.getString("prekeys", null)?.let { unb64(JSONObject(it).getString("signed")) }

    fun oneTimeSecret(id: Int): ByteArray? {
        val raw = prefs.getString("prekeys", null) ?: return null
        val o = JSONObject(raw)
        val ot = o.getJSONObject("one_time")
        if (ot.has(id.toString())) return unb64(ot.getString(id.toString()))
        // Still in the burn pen: a sender working from a head that had not yet
        // propagated. Within the grace window the message is readable; the
        // sweep is what makes the delete real.
        val pen = o.optJSONObject("one_time_burned") ?: return null
        return pen.optJSONObject(id.toString())?.let { unb64(it.getString("sk")) }
    }

    /**
     * Complete the deletes: drop burned one-time secrets past the grace window.
     *
     * This is where §16.11's forward secrecy actually lands. Until this runs,
     * a message sealed to a burned key is still readable on this device —
     * deliberately, for [BURN_GRACE_MS], because head propagation lags — and
     * after it, readable by no one.
     */
    fun sweepBurnedPrekeys() { synchronized(lock) {
        val raw = prefs.getString("prekeys", null) ?: return
        val o = JSONObject(raw)
        val pen = o.optJSONObject("one_time_burned") ?: return
        val cutoff = System.currentTimeMillis() - BURN_GRACE_MS
        val stale = pen.keys().asSequence().toList()
            .filter { (pen.getJSONObject(it).optLong("at")) < cutoff }
        if (stale.isEmpty()) return
        stale.forEach { pen.remove(it) }
        o.put("one_time_burned", pen)
        prefs.edit().putString("prekeys", o.toString()).apply()
    } }

    /**
     * Delete a used one-time secret. This is the operation §16.11's forward
     * secrecy consists of — after it, the message that key opened cannot be
     * opened again by anyone, including us.
     */
    fun burnOneTime(id: Int) { synchronized(lock) {
        val raw = prefs.getString("prekeys", null) ?: return
        val o = JSONObject(raw)
        // The secret moves to a holding pen instead of vanishing. The bundle
        // travels in our log head, which is an eventually-consistent DHT
        // record — a sender's fetch can trail our burn by minutes, so sealing
        // to a just-burned key is a race, not misbehaviour (§16.11). Deleting
        // immediately turns that race into a permanently unreadable message;
        // the pen keeps it readable through the propagation window, and the
        // sweep below completes the delete that forward secrecy consists of.
        val secret = o.getJSONObject("one_time").opt(id.toString())
        o.getJSONObject("one_time").remove(id.toString())
        if (secret != null) {
            val pen = o.optJSONObject("one_time_burned") ?: JSONObject()
            pen.put(id.toString(), JSONObject()
                .put("sk", secret).put("at", System.currentTimeMillis()))
            o.put("one_time_burned", pen)
        }
        // **And prune the published bundle.** Deleting the secret alone leaves
        // the bundle advertising a key that can no longer decrypt anything, and
        // senders take the first one-time entry — so the first key consumed is
        // offered forever and every later message is refused, identically after
        // a re-fetch, because the stale bundle is what gets re-served.
        runCatching {
            uniffi.ducat_mobile.prunePrekey(unb64(o.getString("bundle")), id.toUInt())
        }.onSuccess { o.put("bundle", b64(it)) }
        val e = prefs.edit().putString("prekeys", o.toString())
        // The id lives in exactly one thread's offer; prune it there too, or
        // that head keeps advertising a key that can no longer decrypt.
        prefs.all.keys.filter { it.startsWith("prekeys_ob_") }.forEach { k ->
            val blob = prefs.getString(k, null) ?: return@forEach
            runCatching {
                uniffi.ducat_mobile.prunePrekey(unb64(blob), id.toUInt())
            }.onSuccess { if (!it.contentEquals(unb64(blob))) e.putString(k, b64(it)) }
        }
        e.apply()
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
    /**
     * Where they can be paid without asking first, if they published one.
     *
     * A newer per-request destination supersedes this (§16.12), so a contact
     * who rotates addresses is not undone by the copy we kept.
     */
    val theirAddress: String? = null,
    /**
     * What they published about themselves (§16.9).
     *
     * Their claim, not a finding — nothing here is verified by anything. A
     * screen showing an email beside a persona is showing what that persona
     * said, which is worth having and is not identity.
     */
    val avatar: ByteArray? = null,
    val email: String? = null,
    val phone: String? = null,
    val signal: String? = null,
    val pronouns: Int? = null,
    /** Our log's ring size (§16.12). Eight for logs made before rings grew. */
    val myRing: Int = 8,
    // Their car, from the profile (§15.12): what a rider looks for at the curb.
    val carModel: String? = null,
    val carColor: String? = null,
    val plate: String? = null,
    /** How far into our log they say they have read (§16.16). Their claim. */
    val theirReadUpTo: Long? = null,
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
    /**
     * What to call them, in this order: the name you gave them, the name their
     * card claimed, and — failing both — words rather than their key.
     *
     * A card carries its issuer's name only if they had set one when it was
     * cut, so contacts with neither are ordinary, and every screen used to
     * call those people "2e066ce7…". That is their persona, correctly, and it
     * is also gibberish to read, impossible to say out loud, and impossible to
     * tell from the next one at a glance. [ContactNaming.unnamed] is a
     * placeholder that reads as one, which is the honest thing for a name
     * nobody has supplied — and the app now asks for one when a card arrives
     * without it.
     */
    fun displayName(): String = petname ?: assertedName ?: ContactNaming.unnamed

    /** Whether anyone has actually named them — a prompt worth showing hangs
     *  off this, and so does anything that wants the key instead. */
    val named: Boolean get() = petname != null || assertedName != null

    fun toJson(): JSONObject = JSONObject().apply {
        put("persona", personaHex)
        put("petname", petname ?: JSONObject.NULL)
        put("asserted", assertedName ?: JSONObject.NULL)
        put("my_outbox", myOutbox)
        put("my_outbox_pub", b64(myOutboxOwnerPublic))
        put("my_outbox_sec", b64(myOutboxOwnerSecret))
        put("their_outbox", theirOutbox)
        put("their_bundle", theirBundle?.let { b64(it) } ?: JSONObject.NULL)
        put("their_address", theirAddress ?: JSONObject.NULL)
        put("avatar", avatar?.let { Base64.encodeToString(it, Base64.NO_WRAP) } ?: JSONObject.NULL)
        put("email", email ?: JSONObject.NULL)
        put("phone", phone ?: JSONObject.NULL)
        put("signal", signal ?: JSONObject.NULL)
        put("pronouns", pronouns ?: JSONObject.NULL)
        put("my_ring", myRing)
        put("car_model", carModel ?: JSONObject.NULL)
        put("car_color", carColor ?: JSONObject.NULL)
        put("plate", plate ?: JSONObject.NULL)
        put("their_read", theirReadUpTo ?: JSONObject.NULL)
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
            avatar = o.optStringOrNull("avatar")?.let { Base64.decode(it, Base64.NO_WRAP) },
            email = o.optStringOrNull("email"),
            phone = o.optStringOrNull("phone"),
            signal = o.optStringOrNull("signal"),
            pronouns = if (o.isNull("pronouns")) null else o.optInt("pronouns").takeIf { it in 1..6 },
            myRing = o.optInt("my_ring", 8),
            carModel = o.optStringOrNull("car_model"),
            carColor = o.optStringOrNull("car_color"),
            plate = o.optStringOrNull("plate"),
            theirReadUpTo = if (o.isNull("their_read")) null else o.optLong("their_read"),
            myOutbox = o.optString("my_outbox", ""),
            myOutboxOwnerPublic = unb64(o.optString("my_outbox_pub", "")),
            myOutboxOwnerSecret = unb64(o.optString("my_outbox_sec", "")),
            theirOutbox = o.optString("their_outbox", ""),
            theirBundle = o.optStringOrNull("their_bundle")?.let { unb64(it) },
            theirAddress = o.optStringOrNull("their_address"),
            outSeq = o.optLong("out_seq"),
            outPrevLink = o.optStringOrNull("out_prev")?.let { unb64(it) },
            inSeq = o.optLong("in_seq"),
            inPrevLink = o.optStringOrNull("in_prev")?.let { unb64(it) },
            chatVisible = o.optBoolean("chat_visible", true),
        )
    }
}

/** One line on a bill (§16.13). */
data class BillItem(val description: String, val amountPxmr: Long)

data class StoredMessage(
    val outgoing: Boolean,
    val seq: Long,
    val body: String,
    val timestamp: Long,
    /** 0 text, 1 request, 2 notice, 3 receipt (§16.13). */
    val kind: Int = 0,
    val amountPxmr: Long = 0,
    /** Where a request asks to be paid, if it named one. */
    val payto: String? = null,
    /** For a reaction (§16.14): which message, and in whose log. */
    val reSeq: Long? = null,
    val reOwn: Boolean = false,
    /** An attachment by reference (§16.15); bytes cached by ciphertext hash. */
    val attRecord: String? = null,
    val attKey: ByteArray? = null,
    val attNonce: ByteArray? = null,
    val attLen: Long = 0,
    val attHash: String? = null,
    val attMime: String? = null,
    val attName: String? = null,
    /**
     * The transaction a payment notice points at (§16.13).
     *
     * Advisory — the recipient verifies by finding the output, not by trusting
     * this — but it is the only thing that connects an arriving output to a
     * person. Monero does not carry a sender, so without a notice naming the
     * transaction, "who paid me" has no answer at all.
     */
    val txidHex: String? = null,
    /**
     * What the money was for, line by line (§16.13).
     *
     * Already checked to add up to the amount — core refuses the message
     * otherwise — so a screen rendering this does not have to re-derive the
     * total to know the breakdown is honest arithmetic.
     */
    val items: List<BillItem> = emptyList(),
    val taxPxmr: Long? = null,
    /** False means it went out under the signed prekey — no forward secrecy
     *  until that key rotates (§16.11). Shown, not hidden. */
    val forwardSecret: Boolean = true,
    val delivered: Boolean = true,
    /** A receipt for a bill settled outside DUCAT (§15.11): it names no
     *  transaction because none exists, not because one has yet to be found. */
    val oob: Boolean = false,
    /** §15.12: a ride offer's distance-in-time, seconds. */
    val etaSecs: Long? = null,
) {
    fun toJson(): JSONObject = JSONObject().apply {
        put("out", outgoing); put("seq", seq); put("body", body)
        put("ts", timestamp); put("fs", forwardSecret); put("delivered", delivered)
        put("kind", kind); put("amt", amountPxmr)
        if (oob) put("oob", true)
        put("payto", payto ?: JSONObject.NULL)
        put("txid", txidHex ?: JSONObject.NULL)
        reSeq?.let { put("re_seq", it) }
        if (reOwn) put("re_own", true)
        attRecord?.let {
            put("att_rec", it); put("att_key", Base64.encodeToString(attKey, Base64.NO_WRAP))
            put("att_nonce", Base64.encodeToString(attNonce, Base64.NO_WRAP))
            put("att_len", attLen); put("att_hash", attHash)
            put("att_mime", attMime); put("att_name", attName ?: JSONObject.NULL)
        }
        if (items.isNotEmpty()) {
            put("items", JSONArray().also { a ->
                items.forEach { i ->
                    a.put(JSONObject().put("d", i.description).put("a", i.amountPxmr))
                }
            })
        }
        taxPxmr?.let { put("tax", it) }
        etaSecs?.let { put("eta", it) }
    }

    companion object {
        fun from(o: JSONObject) = StoredMessage(
            outgoing = o.getBoolean("out"),
            seq = o.getLong("seq"),
            body = o.getString("body"),
            timestamp = o.getLong("ts"),
            forwardSecret = o.optBoolean("fs", true),
            delivered = o.optBoolean("delivered", true),
            oob = o.optBoolean("oob", false),
            kind = o.optInt("kind", 0),
            amountPxmr = o.optLong("amt", 0L),
            payto = o.optStringOrNull("payto"),
            txidHex = o.optStringOrNull("txid"),
            reSeq = if (o.has("re_seq")) o.getLong("re_seq") else null,
            reOwn = o.optBoolean("re_own", false),
            attRecord = o.optStringOrNull("att_rec"),
            attKey = o.optStringOrNull("att_key")?.let { Base64.decode(it, Base64.NO_WRAP) },
            attNonce = o.optStringOrNull("att_nonce")?.let { Base64.decode(it, Base64.NO_WRAP) },
            attLen = o.optLong("att_len", 0L),
            attHash = o.optStringOrNull("att_hash"),
            attMime = o.optStringOrNull("att_mime"),
            attName = o.optStringOrNull("att_name"),
            items = o.optJSONArray("items")?.let { a ->
                (0 until a.length()).map {
                    val i = a.getJSONObject(it)
                    BillItem(i.getString("d"), i.getLong("a"))
                }
            } ?: emptyList(),
            taxPxmr = if (o.has("tax")) o.getLong("tax") else null,
            etaSecs = if (o.has("eta")) o.getLong("eta") else null,
        )
    }
}

/** Our own display name, and the last card we issued. */
class NameStore(context: Context) {
    private val prefs = securePrefs(context, "ducat_contacts")
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
    val uri: String = "",
    /** "profile" (the standing code) or "sale" (a till/tab/ride handshake). */
    val purpose: String = "profile",
    val answeredBy: String? = null,
)

class CardStore(context: Context) {
    private val prefs = securePrefs(context, "ducat_contacts")

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
/**
 * The word for a contact nobody has named, in the reader's language.
 *
 * A holder rather than a `getString` at each call site because
 * [Contact.displayName] is a pure function on stored data, reached from
 * twenty-odd screens, from receipts captured in the background and from the
 * desktop client, none of which carry a `Context`. MainActivity sets it before
 * the first screen draws, and Android recreates the activity on a language
 * change, so it follows the chosen language without anything watching for it.
 */
object ContactNaming {
    @Volatile
    var unnamed: String = "Unnamed contact"
}

class PersonaStore(context: Context) {
    private val prefs = securePrefs(context, "ducat_contacts")

    /** Our own persona, in the same hex form contacts are keyed by. */
    fun personaHex(): String =
        uniffi.ducat_mobile.personaPublicHex(secret())

    fun secret(): ByteArray {
        prefs.getString("persona_secret", null)?.let { return unb64(it) }
        val fresh = uniffi.ducat_mobile.createPersonaSecret()
        prefs.edit().putString("persona_secret", b64(fresh)).apply()
        return fresh
    }

    /**
     * Become the identity in a backup.
     *
     * The one write that makes a restore a restore rather than a copy of
     * somebody's address book: contacts are keyed by *their* persona, but every
     * message this device sends is signed by ours, so a device that recovered
     * the threads and kept its own keypair is a stranger to everyone in them.
     * Nothing called this before it existed — `personaSecret` travelled in the
     * bundle, was read, and was never used.
     */
    fun restoreSecret(secret: ByteArray) {
        if (secret.isEmpty()) return
        prefs.edit().putString("persona_secret", b64(secret)).apply()
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
    private val prefs = securePrefs(context, "ducat_contacts")

    fun save(address: String, spendKeyHex: String, restoreHeight: ULong, stagenet: Boolean) {
        prefs.edit()
            .putString("wallet_address", address)
            .putString("wallet_spend", spendKeyHex)
            .putString("wallet_height", restoreHeight.toString())
            .putBoolean("wallet_stagenet", stagenet)
            .apply()
    }

    fun address(): String? = prefs.getString("wallet_address", null)

    // --- per-contact subaddresses (§15.10) --------------------------------
    //
    // One counterparty, one address: a primary handed to everyone is a
    // public ledger entry linking every payment anyone ever made to this
    // person the moment two of them compare notes. Minors allocate once per
    // persona and never move; the scanner watches every allocated minor, so
    // an arriving output names its counterparty by construction instead of
    // by believing a note.

    /** The receiving address for this contact, allocated on first use. */
    fun addressFor(personaHex: String): String? {
        val spend = prefs.getString("wallet_spend", null) ?: return address()
        val stagenet = prefs.getBoolean("wallet_stagenet", true)
        val minor = minorFor(personaHex)
        return runCatching {
            uniffi.ducat_mobile.moneroSubaddress(spend, minor.toUInt(), stagenet)
        }.getOrNull() ?: address()
    }

    /** This contact's minor index, allocated once. */
    fun minorFor(personaHex: String): Int {
        prefs.getInt("sub_minor_$personaHex", 0).takeIf { it != 0 }?.let { return it }
        val next = prefs.getInt("sub_next", 1)
        prefs.edit()
            .putInt("sub_minor_$personaHex", next)
            .putInt("sub_next", next + 1)
            .apply()
        return next
    }

    /** This contact's minor if one was ever allocated — no allocation here. */
    fun minorOf(personaHex: String): Int? =
        prefs.getInt("sub_minor_$personaHex", 0).takeIf { it != 0 }

    /** The scanner's high-water mark: every minor ever allocated. */
    fun subaddressCount(): Int = prefs.getInt("sub_next", 1) - 1

    /** A card's minor becomes its claimant's the moment we learn who that is. */
    fun adoptMinor(cardKey: String, personaHex: String) {
        val m = prefs.getInt("sub_minor_$cardKey", 0)
        if (m != 0 && prefs.getInt("sub_minor_$personaHex", 0) == 0) {
            prefs.edit()
                .putInt("sub_minor_$personaHex", m)
                .remove("sub_minor_$cardKey")
                .apply()
        }
    }

    /** Who an output's receiving minor belongs to, if anyone. */
    fun personaForMinor(minor: Int): String? {
        if (minor == 0) return null
        return prefs.all.keys
            .firstOrNull {
                it.startsWith("sub_minor_") && prefs.getInt(it, 0) == minor
            }?.removePrefix("sub_minor_")
    }

    // --- what happened, not just what arrived --------------------------------

    /**
     * Record a payment we made.
     *
     * Received outputs come from the chain. A payment we *sent* never appears
     * there as anything this wallet can recognise, because the outputs it
     * creates belong to somebody else — so without recording it here, sending
     * money leaves no trace and the balance simply drops.
     */
    fun recordSent(
        txidHex: String,
        amountPxmr: Long,
        feePxmr: Long,
        toAddress: String,
        contactHex: String?,
        note: String?,
    ) {
        val arr = JSONArray(prefs.getString("wallet_sends", "[]"))
        arr.put(JSONObject().apply {
            put("txid", txidHex); put("amt", amountPxmr); put("fee", feePxmr)
            put("to", toAddress); put("contact", contactHex ?: JSONObject.NULL)
            put("note", note ?: JSONObject.NULL)
            put("ts", System.currentTimeMillis() / 1000)
        })
        prefs.edit().putString("wallet_sends", arr.toString()).apply()
        ContactStore.bump()
    }

    fun sends(): List<SentPayment> {
        val arr = JSONArray(prefs.getString("wallet_sends", "[]"))
        return (0 until arr.length()).map {
            val o = arr.getJSONObject(it)
            SentPayment(
                txidHex = o.getString("txid"),
                amountPxmr = o.getLong("amt"),
                feePxmr = o.optLong("fee", 0),
                toAddress = o.optString("to", ""),
                contactHex = if (o.isNull("contact")) null else o.optString("contact"),
                note = if (o.isNull("note")) null else o.optString("note"),
                timestamp = o.optLong("ts", 0),
            )
        }
    }


    // --- scan state -------------------------------------------------------

    /**
     * Bumped when stored outputs become untrustworthy and must be re-read.
     *
     * Version 1: key images were derived as x·P instead of x·H_p(P). Every one
     * was wrong, so the daemon reported genuinely spent outputs as unspent and
     * the wallet counted them again. Fixing the derivation does not fix the
     * entries already written under it — they are keyed by a value that will
     * never match anything again — so they have to go and be rescanned.
     */
    private val OUTPUT_SCHEMA = 1

    /** Returns true if it wiped anything. */
    fun migrateOutputsIfNeeded(): Boolean {
        if (prefs.getInt("wallet_output_schema", 0) >= OUTPUT_SCHEMA) return false
        val had = prefs.getString("wallet_outputs", null) != null
        prefs.edit()
            .remove("wallet_outputs")
            .putLong("wallet_scanned_to", restoreHeight().toLong())
            // Same reason `rescanFrom` clears these, and missing it here made
            // the same mistake: the first window after a wipe gets timed
            // against the clock reading from before it, so 173 blocks that
            // take two minutes were quoted at thirty-four.
            .remove("wallet_rate")
            .remove("wallet_scan_at")
            .remove("wallet_scan_error")
            .putInt("wallet_output_schema", OUTPUT_SCHEMA)
            .apply()
        ContactStore.bump()
        return had
    }

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
                minor = o.minor.toInt(),
                blob = o.blob,
                txHashHex = o.txHashHex,
                timestamp = o.timestamp.toLong(),
            )
        }
        writeEntries(byKi.values.toList())
        prefs.edit()
            .putLong("wallet_scanned_to", scannedTo)
            .putLong("wallet_tip", tip)
            .apply()
    }

    /**
     * Overwrite the output set wholesale.
     *
     * For backfills that add detail to outputs already found — a transaction id
     * recovered from the blob, a block time looked up — rather than for anything
     * that changes what the wallet owns. Callers pass a list derived from
     * [entries]; passing a partial one drops money.
     */
    fun replaceEntries(list: List<WalletEntry>) = writeEntries(list)

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
                txHashHex = o.optString("tx", ""),
                timestamp = o.optLong("ts", 0L),
                minor = o.optInt("minor", 0),
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
                put("tx", it.txHashHex); put("ts", it.timestamp)
                if (it.minor != 0) put("minor", it.minor)
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
    private val prefs = securePrefs(context, "ducat_contacts")

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

/**
 * Which contact arbitrates this device's ride escrows (§15.12): the third
 * key in every 2-of-3 the accept builds. One at a time, chosen by the user
 * from their contacts — until markets carry arbiter descriptors (§10), the
 * arbiter is somebody you already trust enough to hold a tie-breaking share.
 * Nobody is a default: with none set, a hail is the unbonded mutual promise
 * it always was.
 */
class ArbiterStore(context: Context) {
    private val prefs = securePrefs(context, "ducat_contacts")

    fun hex(): String? = prefs.getString("arbiter_hex", null)?.ifBlank { null }

    fun set(personaHex: String?) =
        prefs.edit().putString("arbiter_hex", personaHex ?: "").apply()
}

/** Which Monero node to use, and the last one that worked. */
class NodeStore(context: Context) {
    private val prefs = securePrefs(context, "ducat_contacts")

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

    /** A node call worked: the current node keeps its job. */
    fun nodeSucceeded() = prefs.edit().putInt("monero_node_fails", 0).apply()

    /**
     * A node call failed. Three strikes clears [lastGood] so the next poll
     * cycle re-probes the candidates instead of hammering a dying node
     * forever — which is exactly what a field phone did for nine hours
     * (2026-08-17): scans, fee estimates and finally a send all fed to a
     * node that had stopped answering, because nothing ever demoted it.
     *
     * @return true when this failure demoted the node.
     */
    fun nodeFailed(): Boolean {
        val n = prefs.getInt("monero_node_fails", 0) + 1
        return if (n >= 3) {
            prefs.edit().remove("monero_last_good").putInt("monero_node_fails", 0).apply()
            true
        } else {
            prefs.edit().putInt("monero_node_fails", n).apply()
            false
        }
    }

    /**
     * The node did not answer at all — demote it now rather than on the third
     * try.
     *
     * Three strikes is right for an ambiguous failure, where the node may be
     * fine and the request wrong. A read that times out is not ambiguous, and
     * making someone watch the same payment fail three times before the app
     * quietly tries a different node is three failures too many: it looks like
     * the wallet is broken, not like one server is slow.
     */
    fun nodeUnreachable() =
        prefs.edit().remove("monero_last_good").putInt("monero_node_fails", 0).apply()
}


/** A payment this wallet made. */
data class SentPayment(
    val txidHex: String,
    val amountPxmr: Long,
    val feePxmr: Long,
    val toAddress: String,
    val contactHex: String?,
    val note: String?,
    val timestamp: Long,
)
