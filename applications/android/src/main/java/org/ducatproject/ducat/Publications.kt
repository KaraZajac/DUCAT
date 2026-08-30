package org.ducatproject.ducat

import android.content.Context
import org.json.JSONObject

/**
 * §16.20: publications, both chairs.
 *
 * The subscriber's half is a filing cabinet: period keys arrive as kind-13
 * messages down paid threads, and the spec's SHOULD is implemented as the
 * store's shape — keys file by (publisher persona, period id) and live in
 * their own prefs file, so deleting a conversation does not delete what it
 * paid for. The receipt outlives the small talk; so does the key.
 *
 * The publisher's half is one secret per publication: every period's key
 * derives from the master (core::publish), so there is no keyring to grow
 * stale and a restored phone can cut any back-catalogue key it ever sold.
 */
object Publications {
    private val lock = Any()

    private fun b64(b: ByteArray): String =
        android.util.Base64.encodeToString(b, android.util.Base64.NO_WRAP)
    private fun unb64(s: String): ByteArray =
        android.util.Base64.decode(s, android.util.Base64.NO_WRAP)

    private fun prefs(context: Context) = securePrefs(context, "ducat_publications")

    // --- the subscriber's cabinet -----------------------------------------

    /**
     * File an arriving kind-13. Called from the poll loop's arrival funnel,
     * like the group roster — if it was stored, it was filed.
     *
     * Last write wins per (publisher, period): a publisher re-sending a key
     * is the retry path, and a changed key for an old period is the
     * publisher's own mistake to make — the reader keeps what it is told by
     * the one thread that could tell it.
     */
    fun absorbKey(context: Context, publisherHex: String, m: StoredMessage) {
        val period = m.pubPeriodId ?: return
        val key = m.pubPeriodKey ?: return
        synchronized(lock) {
            val p = prefs(context)
            val all = p.getString("subs", null)?.let { JSONObject(it) } ?: JSONObject()
            val mine = all.optJSONObject(publisherHex) ?: JSONObject()
            val periods = mine.optJSONObject("periods") ?: JSONObject()
            periods.put(period, b64(key))
            mine.put("periods", periods)
            // The shelf rides the first delivery; a later message MAY repeat
            // it (a re-shelved publication), and newest wins for the same
            // reason as the key.
            m.pubRecord?.let { mine.put("record", it) }
            m.pubHeadKey?.let { mine.put("head", b64(it)) }
            all.put(publisherHex, mine)
            p.edit().putString("subs", all.toString()).apply()
        }
        ContactStore.bump()
        DucatLog.i("Publications", "filed period '$period' from ${publisherHex.take(8)}…")
    }

    /** Everything held from one publisher: (record, headKey, periodId → key). */
    fun subscription(
        context: Context,
        publisherHex: String,
    ): Triple<String?, ByteArray?, Map<String, ByteArray>>? {
        val all = prefs(context).getString("subs", null)?.let { JSONObject(it) } ?: return null
        val mine = all.optJSONObject(publisherHex) ?: return null
        val periods = mine.optJSONObject("periods") ?: JSONObject()
        val map = buildMap {
            for (k in periods.keys()) put(k, unb64(periods.getString(k)))
        }
        return Triple(
            mine.optString("record", "").ifBlank { null },
            mine.optString("head", "").ifBlank { null }?.let { unb64(it) },
            map,
        )
    }

    /** Publishers this phone holds keys from, newest filing first not
     *  promised — a cabinet, not a feed. */
    fun subscribedPublishers(context: Context): List<String> {
        val all = prefs(context).getString("subs", null)?.let { JSONObject(it) } ?: return emptyList()
        return all.keys().asSequence().toList()
    }

    // --- the publisher's shelf --------------------------------------------

    /**
     * Start a publication: one master secret, minted here and never shown.
     * Returns its id (hex of the first 8 bytes of its public face — enough
     * to file by, never on the wire).
     */
    fun create(context: Context, title: String): String {
        val master = uniffi.ducat_mobile.publicationMasterCreate()
        val id = master.copyOfRange(0, 8).toHexString()
        synchronized(lock) {
            val p = prefs(context)
            val all = p.getString("pubs", null)?.let { JSONObject(it) } ?: JSONObject()
            all.put(
                id,
                JSONObject()
                    .put("title", title)
                    .put("master", b64(master))
                    .put("created", System.currentTimeMillis() / 1000),
            )
            p.edit().putString("pubs", all.toString()).apply()
        }
        ContactStore.bump()
        return id
    }

    fun publications(context: Context): List<Pair<String, String>> {
        val all = prefs(context).getString("pubs", null)?.let { JSONObject(it) } ?: return emptyList()
        return all.keys().asSequence().map { it to all.getJSONObject(it).optString("title") }.toList()
    }

    /** A period's key, derived — the master never leaves this function's callee. */
    fun periodKey(context: Context, pubId: String, periodId: String): ByteArray? {
        val all = prefs(context).getString("pubs", null)?.let { JSONObject(it) } ?: return null
        val master = all.optJSONObject(pubId)?.optString("master")?.ifBlank { null } ?: return null
        return runCatching {
            uniffi.ducat_mobile.publicationPeriodKey(unb64(master), periodId)
        }.getOrNull()
    }

    /**
     * Hand a period to a subscriber: the kind-13 send, shelf included the
     * first time this thread was ever handed one. The caller decides WHEN —
     * settlement observed, per §15.11's reconcile discipline — this only
     * says what and how.
     */
    fun sendPeriod(
        context: Context,
        c: Contact,
        pubId: String,
        periodId: String,
        record: String?,
        headKey: ByteArray?,
        note: String,
    ): Boolean {
        val key = periodKey(context, pubId, periodId) ?: return false
        val firstTime = ContactStore(context).thread(c.personaHex)
            .none { it.outgoing && it.kind == 13 }
        return runCatching {
            Mailbox.send(
                context, c, note,
                kind = 13,
                pubPeriodId = periodId,
                pubPeriodKey = key,
                pubRecord = if (firstTime) record else null,
                pubHeadKey = if (firstTime) headKey else null,
            )
        }.isSuccess
    }
}
