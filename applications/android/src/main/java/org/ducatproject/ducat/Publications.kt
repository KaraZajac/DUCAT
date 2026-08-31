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
        // An unsubscribed reader stops FILING, not holding: what was paid
        // for stays theirs (§16.20 — no revocation), but a publisher who
        // keeps sending into a closed door does not quietly reopen it.
        if (isMuted(context, publisherHex)) {
            DucatLog.i("Publications", "muted ${publisherHex.take(8)}… — key not filed")
            return
        }
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

    /** The reader's door, closed or open. Muting stops new keys from being
     *  filed and hides the shelf's future — held periods stay held. */
    fun setMuted(context: Context, publisherHex: String, muted: Boolean) {
        synchronized(lock) {
            val p = prefs(context)
            val all = p.getString("subs", null)?.let { JSONObject(it) } ?: JSONObject()
            val mine = all.optJSONObject(publisherHex) ?: JSONObject()
            if (muted) mine.put("muted", true) else mine.remove("muted")
            all.put(publisherHex, mine)
            p.edit().putString("subs", all.toString()).apply()
        }
        ContactStore.bump()
    }

    fun isMuted(context: Context, publisherHex: String): Boolean =
        prefs(context).getString("subs", null)?.let { JSONObject(it) }
            ?.optJSONObject(publisherHex)?.optBoolean("muted", false) ?: false

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
        // The wire refuses an empty body on every kind, so a note-less
        // period rides a protocol-written line — in the SENDER's language,
        // the bill placeholder's own rule. Every automated send (the
        // settle reconcile, send-to-remaining, re-seed) is note-less by
        // nature; without this they all failed with "1 to 2000 characters",
        // silently, forever. Found live by pubsettletest.
        val body = note.trim().ifBlank {
            context.getString(R.string.library_note_new_issue)
        }
        return runCatching {
            Mailbox.send(
                context, c, body,
                kind = 13,
                pubPeriodId = periodId,
                pubPeriodKey = key,
                pubRecord = if (firstTime) record else null,
                pubHeadKey = if (firstTime) headKey else null,
                pubSwarmKey = swarmKey,
                pubSwarmDigestHex = swarmDigestHex,
            )
        }.onFailure {
            // Named, not swallowed: a failing send retried silently forever
            // looks exactly like a reconcile that never ran.
            DucatLog.w("Publications", "sendPeriod '$periodId': ${it.message}")
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

    /** One shipped period, as the log remembers it — by either rail. */
    data class Issue(
        val periodId: String,
        val file: String,
        val swarmKey: String,
        val swarmDigestHex: String,
        val sentTo: Set<String>,
        /** The shelf rail (§16.20): this period's own DHT record. */
        val shelfRec: String = "",
        val shelfChunks: Int = 0,
        val shelfBytes: Long = 0,
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
                shelfRec = o.optString("rec"),
                shelfChunks = o.optInt("rec_chunks", 0),
                shelfBytes = o.optLong("rec_bytes", 0L),
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

    // --- the shelf: the rail that outlives the process --------------------
    //
    // §16.20's first delivery rail: the publication's root record holds an
    // index sealed under the standing head key; each period gets a record
    // of its own, every chunk sealed to its landing site (record key +
    // subkey as AAD, core::publish). A reader with the manifest's pair can
    // fetch at 3am with the publisher's desk dark — the network holds the
    // bytes, the thread only ever carried the capability.
    //
    // Layout (client v1, one implementation family; the spec pins the
    // sealing and deliberately not yet these bytes):
    //   root record, 1 subkey. Subkey 0 = seal_chunk(head, root, 0, nonce,
    //     index-JSON {"v":1,"periods":{id:{"rec","chunks","bytes","name"}}})
    //     — rewritten whole; the publisher is the only writer.
    //   period record, `chunks` subkeys. Subkey i = seal_chunk(periodKey,
    //     rec, i, nonce, plaintext[i]) — 32 KiB values, Veilid's 32-subkey
    //     cap, so the shelf carries up to ~1 MiB; heavier months go by
    //     swarm and the manifest says which truck came.

    /** Plaintext per chunk: a 32 KiB value less nonce (24) and tag (16). */
    const val SHELF_CHUNK_PLAIN = 32_768 - 40
    private const val SHELF_MAX_CHUNKS = 32

    /** What one period record can hold; the Publish room's rail rule. */
    const val SHELF_CAP_BYTES = SHELF_CHUNK_PLAIN.toLong() * SHELF_MAX_CHUNKS

    /** The standing shelf (root record + head key), minted on first use.
     *  Owner keys are kept so restarts and tending can keep writing. */
    fun shelfOf(context: Context, pubId: String): Pair<String, ByteArray>? {
        val pub = readPub(context, pubId) ?: return null
        val rec = pub.optString("root_rec").ifBlank { null }
        val head = pub.optString("head").ifBlank { null }
        if (rec != null && head != null) return rec to unb64(head)
        return null
    }

    private fun ensureShelf(context: Context, pubId: String): Pair<String, ByteArray>? {
        shelfOf(context, pubId)?.let { return it }
        val head = ByteArray(32).also { java.security.SecureRandom().nextBytes(it) }
        val rec = runCatching { uniffi.ducat_mobile.nodeDhtCreate(1u) }.getOrElse {
            DucatLog.w("Publications", "shelf root: ${it.message}")
            return null
        }
        editPub(context, pubId) { pub ->
            pub.put("head", b64(head))
            pub.put("root_rec", rec.key)
            pub.put("root_pub", b64(rec.ownerPublic))
            pub.put("root_sec", b64(rec.ownerSecret))
        }
        return rec.key to head
    }

    private fun sealTo(
        key: ByteArray,
        recordKey: String,
        subkey: Int,
        plain: ByteArray,
    ): ByteArray {
        val nonce = ByteArray(24).also { java.security.SecureRandom().nextBytes(it) }
        return uniffi.ducat_mobile.publicationSealChunk(
            key, recordKey, subkey.toUInt(), nonce, plain,
        )
    }

    /** Open the root for writing (restart-safe: read-first, per the
     *  local-copy rule veilid holds writers to), then rewrite the index. */
    private fun writeIndex(context: Context, pubId: String, mutate: (JSONObject) -> Unit): Boolean {
        val pub = readPub(context, pubId) ?: return false
        val root = pub.optString("root_rec").ifBlank { null } ?: return false
        val head = pub.optString("head").ifBlank { null }?.let { unb64(it) } ?: return false
        val ownPub = pub.optString("root_pub").ifBlank { null }?.let { unb64(it) } ?: return false
        val ownSec = pub.optString("root_sec").ifBlank { null }?.let { unb64(it) } ?: return false
        return runCatching {
            uniffi.ducat_mobile.nodeDhtOpen(root, ownPub, ownSec)
            val existing = runCatching { uniffi.ducat_mobile.nodeDhtGet(root, 0u, true) }.getOrNull()
            val index = existing?.let {
                runCatching {
                    JSONObject(String(
                        uniffi.ducat_mobile.publicationOpenChunk(head, root, 0u, it),
                        Charsets.UTF_8,
                    ))
                }.getOrNull()
            } ?: JSONObject().put("v", 1).put("periods", JSONObject())
            mutate(index)
            uniffi.ducat_mobile.nodeDhtSet(
                root, 0u,
                sealTo(head, root, 0, index.toString().toByteArray(Charsets.UTF_8)),
            )
            true
        }.onFailure {
            DucatLog.w("Publications", "index write: ${it.message}")
        }.getOrDefault(false)
    }

    /**
     * Shelve one period: its own record, every chunk sealed to where it
     * lands, the index updated last — so a reader who finds the period in
     * the index finds every chunk already in place.
     */
    fun shelveIssue(
        context: Context,
        pubId: String,
        periodId: String,
        file: java.io.File,
        onChunk: (Int, Int) -> Unit = { _, _ -> },
    ): Boolean {
        val bytes = file.readBytes()
        if (bytes.isEmpty() || bytes.size > SHELF_CAP_BYTES) {
            DucatLog.w("Publications", "shelve: ${bytes.size} bytes is not shelf-sized")
            return false
        }
        val key = periodKey(context, pubId, periodId) ?: return false
        if (ensureShelf(context, pubId) == null) return false
        val chunks = (bytes.size + SHELF_CHUNK_PLAIN - 1) / SHELF_CHUNK_PLAIN
        val rec = runCatching { uniffi.ducat_mobile.nodeDhtCreate(chunks.toUInt()) }.getOrElse {
            DucatLog.w("Publications", "shelve: ${it.message}")
            return false
        }
        return runCatching {
            for (i in 0 until chunks) {
                val end = minOf((i + 1) * SHELF_CHUNK_PLAIN, bytes.size)
                uniffi.ducat_mobile.nodeDhtSet(
                    rec.key, i.toUInt(),
                    sealTo(key, rec.key, i, bytes.copyOfRange(i * SHELF_CHUNK_PLAIN, end)),
                )
                onChunk(i + 1, chunks)
            }
            check(
                writeIndex(context, pubId) { index ->
                    index.getJSONObject("periods").put(
                        periodId,
                        JSONObject()
                            .put("rec", rec.key)
                            .put("chunks", chunks)
                            .put("bytes", bytes.size)
                            .put("name", file.name),
                    )
                },
            ) { "the index would not write" }
            synchronized(lock) {
                val p = prefs(context)
                val all = p.getString("pubs", null)?.let { JSONObject(it) } ?: return@synchronized
                val pub = all.optJSONObject(pubId) ?: return@synchronized
                val iss = pub.optJSONObject("issues") ?: JSONObject()
                val o = iss.optJSONObject(periodId) ?: JSONObject()
                o.put("file", file.absolutePath)
                    .put("rec", rec.key)
                    .put("rec_chunks", chunks)
                    .put("rec_bytes", bytes.size)
                    .put("rec_pub", b64(rec.ownerPublic))
                    .put("rec_sec", b64(rec.ownerSecret))
                iss.put(periodId, o)
                pub.put("issues", iss)
                all.put(pubId, pub)
                p.edit().putString("pubs", all.toString()).apply()
            }
            ContactStore.bump()
            true
        }.onFailure {
            DucatLog.w("Publications", "shelve '$periodId': ${it.message}")
        }.getOrDefault(false)
    }

    /**
     * The reader's half, shared by the Library and the desk: index by the
     * head key, chunks by the period key, every open naming the landing
     * site it actually read from. Returns the written file.
     */
    fun fetchShelf(
        context: Context,
        publisherHex: String,
        periodId: String,
        outDir: java.io.File,
        onProgress: (Long, Long) -> Unit = { _, _ -> },
    ): java.io.File {
        val sub = subscription(context, publisherHex)
            ?: throw IllegalStateException("no subscription filed")
        val root = sub.first ?: throw IllegalStateException("no shelf on file")
        val head = sub.second ?: throw IllegalStateException("no head key on file")
        val periodKey = sub.third[periodId]
            ?: throw IllegalStateException("no key for '$periodId'")

        uniffi.ducat_mobile.nodeDhtOpen(root, null, null)
        val rawIndex = uniffi.ducat_mobile.nodeDhtGet(root, 0u, true)
            ?: throw IllegalStateException("the shelf's index is not on the network")
        val index = JSONObject(String(
            uniffi.ducat_mobile.publicationOpenChunk(head, root, 0u, rawIndex),
            Charsets.UTF_8,
        ))
        val entry = index.optJSONObject("periods")?.optJSONObject(periodId)
            ?: throw IllegalStateException("'$periodId' is not on the shelf yet")
        val rec = entry.getString("rec")
        val chunks = entry.getInt("chunks")
        val total = entry.getLong("bytes")
        val name = entry.optString("name").ifBlank { "issue.bin" }

        uniffi.ducat_mobile.nodeDhtOpen(rec, null, null)
        outDir.mkdirs()
        val out = java.io.File(outDir, name)
        java.io.FileOutputStream(out).use { fos ->
            var done = 0L
            for (i in 0 until chunks) {
                onProgress(done, total)
                val value = uniffi.ducat_mobile.nodeDhtGet(rec, i.toUInt(), true)
                    ?: throw IllegalStateException("chunk $i is missing from the shelf")
                val plain = uniffi.ducat_mobile.publicationOpenChunk(
                    periodKey, rec, i.toUInt(), value,
                )
                fos.write(plain)
                done += plain.size
            }
            onProgress(done, total)
        }
        return out
    }

    /**
     * Keep the catalogue breathing: rewrite the index and touch each
     * period record so no TTL quietly eats a back-catalogue. Bounded to
     * once an hour; rides the poll clock beside the reconcilers.
     */
    fun tendShelf(context: Context) {
        val p = prefs(context)
        val last = p.getLong("shelf_tended", 0L)
        val now = System.currentTimeMillis()
        if (now - last < 60 * 60_000L) return
        p.edit().putLong("shelf_tended", now).apply()
        for ((pubId, _) in publications(context)) {
            if (shelfOf(context, pubId) == null) continue
            writeIndex(context, pubId) { /* a rewrite is the point */ }
            val pub = readPub(context, pubId) ?: continue
            val iss = pub.optJSONObject("issues") ?: continue
            for (period in iss.keys().asSequence().toList()) {
                val o = iss.getJSONObject(period)
                val rec = o.optString("rec").ifBlank { null } ?: continue
                val rp = o.optString("rec_pub").ifBlank { null }?.let { unb64(it) } ?: continue
                val rs = o.optString("rec_sec").ifBlank { null }?.let { unb64(it) } ?: continue
                runCatching {
                    uniffi.ducat_mobile.nodeDhtOpen(rec, rp, rs)
                    val v = uniffi.ducat_mobile.nodeDhtGet(rec, 0u, true) ?: return@runCatching
                    uniffi.ducat_mobile.nodeDhtSet(rec, 0u, v)
                }.onFailure {
                    DucatLog.w("Publications", "tend '$period': ${it.message}")
                }
            }
        }
    }

    // --- scan-to-subscribe (§16.20 meets §16.9's cards) -------------------
    //
    // A publish-purpose card IS the subscription form: the Publishing
    // screen mints one per publication and remembers which shelf it opens;
    // when somebody claims it, collectClaims enrolls them — and a free
    // publication hands the newcomer the latest issue on the spot, because
    // scanning should feel like getting the paper, not applying for it. A
    // priced one bills them the newest period through the same TabStore
    // rails, and the settle reconcile delivers when the chain says so.

    /** Remember which publication a minted publish-card opens. */
    fun bindCard(context: Context, pubId: String, inboxKey: String) {
        synchronized(lock) {
            val p = prefs(context)
            val map = p.getString("subcards", null)?.let { JSONObject(it) } ?: JSONObject()
            map.put(inboxKey, pubId)
            p.edit().putString("subcards", map.toString()).apply()
        }
    }

    /** The claim side of the card above; called from the claims funnel. */
    fun enrollFromCard(context: Context, inboxKey: String, subscriberHex: String) {
        val pubId = prefs(context).getString("subcards", null)
            ?.let { JSONObject(it) }?.optString(inboxKey)?.ifBlank { null } ?: return
        setSubscriber(context, pubId, subscriberHex, true)
        DucatLog.i(
            "Publications",
            "card claim enrolled ${subscriberHex.take(8)}… into '$pubId'",
        )
        val c = ContactStore(context).all()
            .firstOrNull { it.personaHex == subscriberHex } ?: return
        if (priceOf(context, pubId) > 0L) {
            // Bill the newcomer for the newest period (or this month when
            // nothing has shipped yet). billPeriod skips the already-billed,
            // so only the new arrival gets paper.
            val period = issues(context, pubId).firstOrNull()?.periodId
                ?: java.time.YearMonth.now().toString()
            billPeriod(context, pubId, period)
        } else {
            val latest = issues(context, pubId).firstOrNull() ?: return
            val shelf = shelfOf(context, pubId)
            val ok = sendPeriod(
                context, c, pubId, latest.periodId,
                record = shelf?.first, headKey = shelf?.second, note = "",
                swarmKey = latest.swarmKey.takeIf { it.isNotBlank() },
                swarmDigestHex = latest.swarmDigestHex.takeIf { it.isNotBlank() },
            )
            if (ok) markSent(context, pubId, latest.periodId, subscriberHex)
        }
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
                // Due only once SOME rail exists — pay-then-ship holds
                // until the bytes are reachable, by shelf or by swarm.
                if (issue.swarmKey.isBlank() && issue.shelfRec.isBlank()) continue
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
            // The shelf pair rides the thread's first manifest (sendPeriod
            // gates it); the shipment rides only when this period went by
            // swarm. Both rails on one manifest is the spec's own shape.
            val shelf = shelfOf(context, d.pubId)
            val ok = sendPeriod(
                context, c, d.pubId, d.periodId,
                record = shelf?.first, headKey = shelf?.second, note = "",
                swarmKey = issue.swarmKey.takeIf { it.isNotBlank() },
                swarmDigestHex = issue.swarmDigestHex.takeIf { it.isNotBlank() },
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
