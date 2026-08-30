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
            // The shipment files under its period: the fetch needs exactly
            // this pair, and nothing else may substitute for the digest.
            if (m.pubSwarmKey != null && m.pubSwarmDigest != null) {
                val ships = mine.optJSONObject("ships") ?: JSONObject()
                ships.put(period, JSONObject()
                    .put("key", m.pubSwarmKey).put("digest", m.pubSwarmDigest))
                mine.put("ships", ships)
            }
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
        /** A heavy period's shipment (§16.20): swarm share key + digest. */
        swarmKey: String? = null,
        swarmDigestHex: String? = null,
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
                pubSwarmKey = swarmKey,
                pubSwarmDigestHex = swarmDigestHex,
            )
        }.isSuccess
    }

    /** A period's filed shipment, if one arrived: (shareKey, digestHex). */
    fun shipment(context: Context, publisherHex: String, periodId: String): Pair<String, String>? {
        val all = prefs(context).getString("subs", null)?.let { JSONObject(it) } ?: return null
        val ship = all.optJSONObject(publisherHex)?.optJSONObject("ships")
            ?.optJSONObject(periodId) ?: return null
        return ship.getString("key") to ship.getString("digest")
    }

    // --- the publisher's ledger of who and what ---------------------------
    //
    // The roster is the operator's own list — §16.20 has no subscription
    // object on the wire, deliberately: who gets a period is a decision made
    // at the desk (settlement observed, a comp, a trial), not a protocol
    // fact. The issue log exists because serving dies with the process: a
    // relaunch must know what was shipped, to whom, and from which file, or
    // the operator is re-deriving their own history from chat threads.

    private fun editPub(context: Context, pubId: String, f: (JSONObject) -> Unit): Boolean {
        synchronized(lock) {
            val p = prefs(context)
            val all = p.getString("pubs", null)?.let { JSONObject(it) } ?: return false
            val pub = all.optJSONObject(pubId) ?: return false
            f(pub)
            all.put(pubId, pub)
            p.edit().putString("pubs", all.toString()).apply()
        }
        ContactStore.bump()
        return true
    }

    private fun readPub(context: Context, pubId: String): JSONObject? =
        prefs(context).getString("pubs", null)?.let { JSONObject(it) }?.optJSONObject(pubId)

    /** Who this publication goes to, by persona hex. */
    fun subscribers(context: Context, pubId: String): List<String> {
        val arr = readPub(context, pubId)?.optJSONArray("subs") ?: return emptyList()
        return (0 until arr.length()).map { arr.getString(it) }
    }

    fun setSubscriber(context: Context, pubId: String, personaHex: String, on: Boolean) {
        editPub(context, pubId) { pub ->
            val now = pub.optJSONArray("subs")?.let { a ->
                (0 until a.length()).map { a.getString(it) }
            }.orEmpty().toMutableSet()
            if (on) now.add(personaHex) else now.remove(personaHex)
            pub.put("subs", org.json.JSONArray(now.toList()))
        }
    }

    /** One shipped period, as the log remembers it. */
    data class Issue(
        val periodId: String,
        val file: String,
        val swarmKey: String,
        val swarmDigestHex: String,
        val sentTo: Set<String>,
    )

    /** The issue log, newest period first. */
    fun issues(context: Context, pubId: String): List<Issue> {
        val iss = readPub(context, pubId)?.optJSONObject("issues") ?: return emptyList()
        return iss.keys().asSequence().map { period ->
            val o = iss.getJSONObject(period)
            val sent = o.optJSONArray("sent")?.let { a ->
                (0 until a.length()).map { a.getString(it) }.toSet()
            } ?: emptySet()
            Issue(
                periodId = period,
                file = o.optString("file"),
                swarmKey = o.optString("key"),
                swarmDigestHex = o.optString("digest"),
                sentTo = sent,
            )
        }.sortedByDescending { it.periodId }.toList()
    }

    /** File a freshly seeded period. Re-recording a period replaces its
     *  shipment (a re-seed after relaunch mints a fresh share) and keeps
     *  the sent list — those sends happened. */
    fun recordIssue(
        context: Context,
        pubId: String,
        periodId: String,
        file: String,
        swarmKey: String,
        swarmDigestHex: String,
    ): Boolean = editPub(context, pubId) { pub ->
        val iss = pub.optJSONObject("issues") ?: JSONObject()
        val o = iss.optJSONObject(periodId) ?: JSONObject()
        o.put("file", file).put("key", swarmKey).put("digest", swarmDigestHex)
        iss.put(periodId, o)
        pub.put("issues", iss)
    }

    /** Tab origin for subscription bills — display machinery only, like
     *  "bar" and "taxi"; settlement is TabStore's, untouched. */
    const val ORIGIN = "pub"

    /** The asking price per period, in piconero. Zero = not set. */
    fun priceOf(context: Context, pubId: String): Long =
        readPub(context, pubId)?.optLong("price", 0L) ?: 0L

    fun setPrice(context: Context, pubId: String, pxmr: Long) {
        editPub(context, pubId) { it.put("price", pxmr) }
    }

    /** Tabs billed for a period: personaHex → tab id. */
    fun billedFor(context: Context, pubId: String, periodId: String): Map<String, String> {
        val o = readPub(context, pubId)?.optJSONObject("issues")
            ?.optJSONObject(periodId)?.optJSONObject("billed") ?: return emptyMap()
        return o.keys().asSequence().associateWith { o.getString(it) }
    }

    fun recordBilled(context: Context, pubId: String, periodId: String, personaHex: String, tabId: String) {
        editPub(context, pubId) { pub ->
            val iss = pub.optJSONObject("issues") ?: JSONObject()
            val o = iss.optJSONObject(periodId) ?: JSONObject()
            val billed = o.optJSONObject("billed") ?: JSONObject()
            billed.put(personaHex, tabId)
            o.put("billed", billed)
            iss.put(periodId, o)
            pub.put("issues", iss)
        }
    }

    /**
     * Bill the roster for a period: one ordinary tab per subscriber, on the
     * same rails as a bar tab — key-image snapshot, subaddress match, the
     * receipt from TabStore.reconcile when the chain shows the money.
     * Idempotent per (period, subscriber); returns how many bills went out.
     */
    fun billPeriod(context: Context, pubId: String, periodId: String): Int {
        val price = priceOf(context, pubId)
        if (price <= 0L) return 0
        val title = publications(context).firstOrNull { it.first == pubId }?.second ?: return 0
        val already = billedFor(context, pubId, periodId).keys
        val store = TabStore(context)
        val contacts = ContactStore(context).all().associateBy { it.personaHex }
        var sent = 0
        for (hex in subscribers(context, pubId)) {
            if (hex in already || contacts[hex] == null) continue
            val opened = store.open(hex, ORIGIN)
            val lined = store.mutate(opened.id) {
                it.copy(lines = listOf(BillItem("$title — $periodId", price)))
            } ?: continue
            runCatching { store.settle(lined) }
                .onSuccess {
                    recordBilled(context, pubId, periodId, hex, opened.id)
                    sent++
                }
                .onFailure {
                    // The bill never left; the open tab must not lie in wait
                    // for an unrelated payment of the same figure.
                    store.delete(opened.id)
                    DucatLog.w("Publications", "bill to ${hex.take(8)}… failed: ${it.message}")
                }
        }
        return sent
    }

    /** A settled subscriber owed their issue. */
    data class Due(val pubId: String, val periodId: String, val personaHex: String)

    /**
     * Who has paid and not yet been sent the period — computed from the
     * publisher's own ledger and the tab's settled state, never from the
     * payer's messages (§16.13: a notice is a claim; the tab goes "paid"
     * on the chain's word). Only periods with a recorded shipment are due:
     * pay-then-ship holds the send until the seed exists, and the next
     * reconcile pass delivers.
     */
    fun dueSettled(context: Context): List<Due> {
        val tabs = TabStore(context)
        val out = mutableListOf<Due>()
        for ((pubId, _) in publications(context)) {
            for (issue in issues(context, pubId)) {
                if (issue.swarmKey.isBlank()) continue
                for ((hex, tabId) in billedFor(context, pubId, issue.periodId)) {
                    if (hex in issue.sentTo) continue
                    val t = tabs.get(tabId) ?: continue
                    if (t.state.startsWith("paid")) {
                        out.add(Due(pubId, issue.periodId, hex))
                    }
                }
            }
        }
        return out
    }

    /**
     * The settle→send glue (§15.11's reconcile discipline): runs on the
     * poll clock beside TabStore.reconcile, so a payment landing while the
     * operator sleeps still delivers the issue. Idempotent through
     * [markSent]; a failed send stays due and the next pass retries.
     */
    fun reconcileSettled(context: Context) {
        val due = dueSettled(context)
        if (due.isEmpty()) return
        val contacts = ContactStore(context).all().associateBy { it.personaHex }
        for (d in due) {
            val c = contacts[d.personaHex] ?: continue
            val issue = issues(context, d.pubId).firstOrNull { it.periodId == d.periodId } ?: continue
            val ok = sendPeriod(
                context, c, d.pubId, d.periodId,
                record = null, headKey = null, note = "",
                swarmKey = issue.swarmKey,
                swarmDigestHex = issue.swarmDigestHex,
            )
            if (ok) {
                markSent(context, d.pubId, d.periodId, d.personaHex)
                DucatLog.i(
                    "Publications",
                    "settled → sent '${d.periodId}' to ${d.personaHex.take(8)}…",
                )
            }
        }
    }

    fun markSent(context: Context, pubId: String, periodId: String, personaHex: String) {
        editPub(context, pubId) { pub ->
            val iss = pub.optJSONObject("issues") ?: return@editPub
            val o = iss.optJSONObject(periodId) ?: return@editPub
            val sent = o.optJSONArray("sent")?.let { a ->
                (0 until a.length()).map { a.getString(it) }
            }.orEmpty().toMutableSet()
            sent.add(personaHex)
            o.put("sent", org.json.JSONArray(sent.toList()))
            iss.put(periodId, o)
            pub.put("issues", iss)
        }
    }
}
