package org.ducatproject.ducat

import android.content.Context
import android.util.Base64
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

    fun all(): List<Contact> {
        val raw = prefs.getString("contacts", null) ?: return emptyList()
        val arr = JSONArray(raw)
        return (0 until arr.length()).map { Contact.from(arr.getJSONObject(it)) }
    }

    fun add(c: Contact) {
        val existing = all().filterNot { it.personaHex == c.personaHex }
        save(existing + c)
    }

    fun update(c: Contact) = add(c)

    fun remove(personaHex: String) = save(all().filterNot { it.personaHex == personaHex })

    private fun save(list: List<Contact>) {
        val arr = JSONArray()
        list.forEach { arr.put(it.toJson()) }
        prefs.edit().putString("contacts", arr.toString()).apply()
    }

    // --- threads ----------------------------------------------------------

    fun thread(personaHex: String): List<StoredMessage> {
        val raw = prefs.getString("thread_$personaHex", null) ?: return emptyList()
        val arr = JSONArray(raw)
        return (0 until arr.length()).map { StoredMessage.from(arr.getJSONObject(it)) }
    }

    fun append(personaHex: String, m: StoredMessage) {
        val arr = JSONArray()
        (thread(personaHex) + m).forEach { arr.put(it.toJson()) }
        prefs.edit().putString("thread_$personaHex", arr.toString()).apply()
    }

    // --- prekeys ----------------------------------------------------------

    /** Our own published bundle and its secrets. */
    fun savePrekeys(bundle: ByteArray, signedSecret: ByteArray, oneTime: Map<Int, ByteArray>) {
        val o = JSONObject()
        o.put("bundle", b64(bundle))
        o.put("signed", b64(signedSecret))
        val ot = JSONObject()
        oneTime.forEach { (id, sk) -> ot.put(id.toString(), b64(sk)) }
        o.put("one_time", ot)
        prefs.edit().putString("prekeys", o.toString()).apply()
    }

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
    fun burnOneTime(id: Int) {
        val raw = prefs.getString("prekeys", null) ?: return
        val o = JSONObject(raw)
        o.getJSONObject("one_time").remove(id.toString())
        prefs.edit().putString("prekeys", o.toString()).apply()
    }

    fun oneTimeRemaining(): Int {
        val raw = prefs.getString("prekeys", null) ?: return 0
        return JSONObject(raw).getJSONObject("one_time").length()
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
