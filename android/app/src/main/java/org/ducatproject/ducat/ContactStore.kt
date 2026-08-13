package org.ducatproject.ducat

import android.content.Context
import android.util.Base64
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
        return (0 until arr.length()).map { Contact.from(arr.getJSONObject(it)) }
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
    val rendezvous: ByteArray,
    val claimSecret: ByteArray,
    /** Their published prekeys, once fetched. */
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
        put("rendezvous", b64(rendezvous))
        put("claim_secret", b64(claimSecret))
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
            rendezvous = unb64(o.getString("rendezvous")),
            claimSecret = unb64(o.getString("claim_secret")),
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
    /** False means it went out under the signed prekey — no forward secrecy
     *  until that key rotates (§16.11). Shown, not hidden. */
    val forwardSecret: Boolean = true,
    val delivered: Boolean = true,
) {
    fun toJson(): JSONObject = JSONObject().apply {
        put("out", outgoing); put("seq", seq); put("body", body)
        put("ts", timestamp); put("fs", forwardSecret); put("delivered", delivered)
    }

    companion object {
        fun from(o: JSONObject) = StoredMessage(
            outgoing = o.getBoolean("out"),
            seq = o.getLong("seq"),
            body = o.getString("body"),
            timestamp = o.getLong("ts"),
            forwardSecret = o.optBoolean("fs", true),
            delivered = o.optBoolean("delivered", true),
        )
    }
}

/** Our own display name, and the last card we issued. */
class NameStore(context: Context) {
    private val prefs = context.getSharedPreferences("ducat_contacts", Context.MODE_PRIVATE)
    fun get(): String? = prefs.getString("my_name", null)
    fun put(v: String) = prefs.edit().putString("my_name", v).apply()
}

class CardStore(context: Context) {
    private val prefs = context.getSharedPreferences("ducat_contacts", Context.MODE_PRIVATE)

    /** Kept so a second claim on the same card can be refused (§16.9). */
    fun remember(card: uniffi.ducat_mobile.IssuedCard) {
        prefs.edit()
            .putString("my_card", b64(card.bytes))
            .putString("my_card_commit", b64(card.claimCommit))
            .putBoolean("my_card_claimed", false)
            .apply()
    }

    fun cardBytes(): ByteArray? = prefs.getString("my_card", null)?.let { unb64(it) }
    fun claimed(): Boolean = prefs.getBoolean("my_card_claimed", false)
    fun markClaimed() = prefs.edit().putBoolean("my_card_claimed", true).apply()
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
    fun spendKeyHex(): String? = prefs.getString("wallet_spend", null)
    fun stagenet(): Boolean = prefs.getBoolean("wallet_stagenet", true)
    fun restoreHeight(): ULong =
        prefs.getString("wallet_height", null)?.toULongOrNull() ?: 0uL
}
