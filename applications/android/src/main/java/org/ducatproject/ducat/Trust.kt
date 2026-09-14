package org.ducatproject.ducat

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/**
 * §9.5 — costly identity: burning XMR under a persona, keeping the proof,
 * and checking a stranger's.
 *
 * A burn is an ordinary send to the one address nobody holds the keys to
 * ([uniffi.ducat_mobile.moneroBurnAddress], derived from a hash so that
 * anyone can recompute it and see no secret was ever chosen). The send path
 * hands back the transaction's secret key, and from that key and the
 * persona's name this makes Monero's `OutProofV2` **at once** — the key is
 * not kept anywhere a later screen could find it, so a proof not made now is
 * a proof never made. The block height arrives with the chain, minutes
 * later, and only then is the `BURN_PROOF` envelope signed and kept: a burn
 * without a block is not a burn yet.
 *
 * Checking a stranger's envelope is §9.5's three questions, in order, and
 * the order matters because each is cheaper than the next:
 *
 * 1. the proof's message must name **the persona presenting it** — a proof
 *    lifted from somebody else verifies under their name, not this one;
 * 2. our own node must bear the proof out, **for exactly the amount the
 *    proof proves**, never the amount claimed beside it;
 * 3. a second-opinion node must have that transaction **in a block**.
 *    *Unknown*, *in the pool* and *unreachable* are all "not yet" (§17.5's
 *    three answers, the rule the money path already follows) — the one thing
 *    none of them is, is *yes*.
 *
 * Nothing here is published. What a persona shows, it shows in a thread; the
 * verdict is kept beside the contact and never expires, because a burn
 * cannot be undone.
 *
 * This is the phone's half of the desk's `app/src/trust.rs`, deliberately
 * the same rule down to the refusals, because two clients that priced
 * identity differently would be two protocols.
 */
object Trust {

    private const val TAG = "Trust"

    /** Its own file: the envelopes are a persona's identity, not a setting. */
    private const val PREFS = "ducat_trust"
    private const val OURS = "burns"
    private const val THEIRS = "verified"

    /**
     * A burn under this counts for nothing — 0.01 XMR, about the price of a
     * coffee. `BURN_FLOOR_PXMR` in `core/src/trust.rs`, restated here
     * because §9.5 makes it a client policy rather than a wire rule: it can
     * move without a draft, and the gate, not the floor, is what protects
     * money.
     */
    const val FLOOR_PXMR = 10_000_000_000L

    /** A purpose is a label — "identity", "listing", "arbiter". */
    const val MAX_PURPOSE_CHARS = 32

    /** What a purpose says when nobody typed one. */
    const val DEFAULT_PURPOSE = "identity"

    /**
     * Longer than the till's eight seconds: this asks a node for a whole
     * transaction and checks a proof against it, and it runs on the sweep
     * with nobody waiting at a counter.
     */
    private const val NODE_TIMEOUT_MS = 20_000u

    /**
     * The domain in every burn message, and in the burn address's own
     * derivation (§9.5). ASCII, and byte-identical to `BURN_DOMAIN` in
     * `core/src/trust.rs` — the message is signed by Monero's proof, so a
     * client that spelled it differently would produce proofs no other
     * client could check.
     */
    private val BURN_DOMAIN = "DUCAT-BURN-v1".toByteArray(Charsets.US_ASCII)

    /**
     * Two writers, one list. The screen appends a burn while the sweep is
     * asking a node about the last one, and both write the whole list back —
     * so the read-modify-write is held here rather than being a race that
     * loses a proof somebody paid for.
     */
    private val lock = Any()

    /** One of this phone's own burns. */
    data class BurnRecord(
        val personaHex: String,
        val txidHex: String,
        val amountPxmr: Long,
        val purpose: String,
        /** The `OutProofV2`, made at send time. */
        val proof: String,
        /** The block it is in; zero until the chain says. */
        val height: Long,
        /** The signed `BURN_PROOF` envelope, hex — written once there is a block. */
        val envelopeHex: String?,
        val madeAt: Long,
    ) {
        /** A burn with its block, its envelope, and nothing left to wait for. */
        val finished: Boolean get() = !envelopeHex.isNullOrBlank() && height > 0
    }

    /** A stranger's burn, checked and kept beside the contact. */
    data class VerifiedBurn(
        val personaHex: String,
        val txidHex: String,
        val amountPxmr: Long,
        val height: Long,
        val purpose: String,
        val checkedAt: Long,
    )

    /**
     * Why a burn was refused before any money moved.
     *
     * An enum rather than a sentence so the rule can be tested without a
     * `Context`; the wording is chosen at the one call that has one.
     */
    enum class Refusal { UnderFloor, NoPurpose, PurposeTooLong }

    // ----- the rule ------------------------------------------------------------
    //
    // Pure Kotlin, no Android, no network — what a burn must look like before
    // it is one, and the bytes its proof is bound to.

    /**
     * What is wrong with burning [amountPxmr] for [purpose], or null.
     *
     * The floor is a refusal and not a nudge: a burn under it is money
     * destroyed for a proof no reader will accept, which is the one outcome
     * this screen must never produce.
     */
    internal fun refusal(amountPxmr: Long, purpose: String): Refusal? {
        val p = purpose.trim()
        return when {
            amountPxmr < FLOOR_PXMR -> Refusal.UnderFloor
            p.isEmpty() -> Refusal.NoPurpose
            p.codePointCount(0, p.length) > MAX_PURPOSE_CHARS -> Refusal.PurposeTooLong
            else -> null
        }
    }

    /**
     * The message the `OutProofV2` signs: the domain, the persona, the
     * purpose, separated by zero bytes — `burn_message` in
     * `core/src/trust.rs`, byte for byte.
     *
     * This is what makes a burn *this persona's* rather than anyone's: a
     * proof lifted from one persona verifies under another's name only if
     * the message is rewritten, and rewriting it breaks Monero's signature.
     */
    internal fun burnMessage(personaHex: String, purpose: String): ByteArray {
        val persona = unhex(personaHex)
            ?: throw IllegalArgumentException("persona is not hex")
        val label = purpose.trim().toByteArray(Charsets.UTF_8)
        val out = ByteArray(BURN_DOMAIN.size + 1 + persona.size + 1 + label.size)
        var at = 0
        BURN_DOMAIN.copyInto(out, at); at += BURN_DOMAIN.size
        out[at++] = 0
        persona.copyInto(out, at); at += persona.size
        out[at++] = 0
        label.copyInto(out, at)
        return out
    }

    internal fun unhex(h: String): ByteArray? {
        val s = h.trim()
        if (s.length % 2 != 0 || !s.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }) {
            return null
        }
        return ByteArray(s.length / 2) { s.substring(it * 2, it * 2 + 2).toInt(16).toByte() }
    }

    internal fun hex(b: ByteArray): String = b.joinToString("") { "%02x".format(it) }

    // ----- the store -----------------------------------------------------------

    private fun prefs(context: Context) = securePrefs(context, PREFS)

    /** The burn address for this wallet's network, as text. */
    fun burnAddress(context: Context): String =
        uniffi.ducat_mobile.moneroBurnAddress(WalletStore(context).stagenet())

    /**
     * Every burn this phone has made, in the order it made them.
     *
     * Unreadable JSON answers with an empty list and a line in the log
     * rather than a throw: this is read on the wallet screen, and a store
     * nobody can parse is already lost — taking the money screen down with
     * it recovers nothing.
     */
    fun burns(context: Context): List<BurnRecord> = runCatching {
        val arr = JSONArray(prefs(context).getString(OURS, "[]"))
        (0 until arr.length()).map {
            val o = arr.getJSONObject(it)
            BurnRecord(
                personaHex = o.getString("persona"),
                txidHex = o.getString("txid"),
                amountPxmr = o.getLong("amount"),
                purpose = o.optString("purpose", DEFAULT_PURPOSE),
                proof = o.optString("proof", ""),
                height = o.optLong("height", 0L),
                envelopeHex = o.optString("envelope", "").ifBlank { null },
                madeAt = o.optLong("made", 0L),
            )
        }
    }.onFailure { DucatLog.w(TAG, "the burn store did not parse: ${it.message}") }
        .getOrDefault(emptyList())

    private fun putBurns(context: Context, list: List<BurnRecord>) {
        val arr = JSONArray()
        list.forEach { b ->
            arr.put(
                JSONObject().apply {
                    put("persona", b.personaHex)
                    put("txid", b.txidHex)
                    put("amount", b.amountPxmr)
                    put("purpose", b.purpose)
                    put("proof", b.proof)
                    put("height", b.height)
                    b.envelopeHex?.let { put("envelope", it) }
                    put("made", b.madeAt)
                },
            )
        }
        prefs(context).edit().putString(OURS, arr.toString()).apply()
        ContactStore.bump()
    }

    /**
     * The largest finished burn a persona of ours holds, if any.
     *
     * Largest, not newest, and never a sum: §9.5 scores a persona from its
     * single largest burn so that many small identities do not add up to one
     * large one (JoinMarket's reason).
     */
    fun myBurn(context: Context, personaHex: String): BurnRecord? =
        burns(context)
            .filter { it.personaHex.equals(personaHex, ignoreCase = true) && it.finished }
            .maxByOrNull { it.amountPxmr }

    fun verifiedBurns(context: Context): List<VerifiedBurn> = runCatching {
        val arr = JSONArray(prefs(context).getString(THEIRS, "[]"))
        (0 until arr.length()).map {
            val o = arr.getJSONObject(it)
            VerifiedBurn(
                personaHex = o.getString("persona"),
                txidHex = o.getString("txid"),
                amountPxmr = o.getLong("amount"),
                height = o.getLong("height"),
                purpose = o.optString("purpose", DEFAULT_PURPOSE),
                checkedAt = o.optLong("checked", 0L),
            )
        }
    }.onFailure { DucatLog.w(TAG, "the verified-burn store did not parse: ${it.message}") }
        .getOrDefault(emptyList())

    /** What we have verified about a persona's burn, if anything. */
    fun burnOf(context: Context, personaHex: String): VerifiedBurn? =
        verifiedBurns(context)
            .filter { it.personaHex.equals(personaHex, ignoreCase = true) }
            .maxByOrNull { it.amountPxmr }

    private fun putVerified(context: Context, list: List<VerifiedBurn>) {
        val arr = JSONArray()
        list.forEach { v ->
            arr.put(
                JSONObject().apply {
                    put("persona", v.personaHex)
                    put("txid", v.txidHex)
                    put("amount", v.amountPxmr)
                    put("height", v.height)
                    put("purpose", v.purpose)
                    put("checked", v.checkedAt)
                },
            )
        }
        prefs(context).edit().putString(THEIRS, arr.toString()).apply()
        ContactStore.bump()
    }

    // ----- burning -------------------------------------------------------------

    /**
     * Burn [amountPxmr] under [personaHex] for [purpose]: send it to the burn
     * address, make the proof from the transaction key at once, and keep the
     * record. Blocking; call it off the main thread.
     *
     * The proof is made *after* the send and *before* anything else can fail,
     * because the transaction key exists nowhere but the send's own return —
     * [WalletStore.resolveSendIntent] keeps it on the row, and a proof made
     * later is a proof made from a stored copy nobody has yet needed. Should
     * the proof still fail, the record is written without one rather than
     * dropped: the money is gone either way, and a row saying so is how
     * somebody finds out.
     */
    fun burn(context: Context, personaHex: String, amountPxmr: Long, purpose: String): BurnRecord {
        refusal(amountPxmr, purpose)?.let { throw IllegalStateException(words(context, it)) }
        val label = purpose.trim()
        // The persona must be one this phone can sign for, checked before the
        // money moves rather than on the lap that comes after it: a burn
        // nothing here can sign is XMR destroyed for an envelope that will
        // never exist.
        if (PersonaStore(context).secretFor(personaHex) == null) {
            throw IllegalStateException(context.getString(R.string.burn_err_no_persona))
        }
        val message = burnMessage(personaHex, label)
        val stagenet = WalletStore(context).stagenet()
        val address = uniffi.ducat_mobile.moneroBurnAddress(stagenet)
        val node = nodeInUse(context, probe = true)
            ?: throw IllegalStateException(context.getString(R.string.burn_err_no_node))

        val sent = Wallet.send(context, node, address, amountPxmr, note = "burn")
        val proof = runCatching {
            uniffi.ducat_mobile.moneroMakeOutProof(
                sent.txidHex, message, listOf(sent.txKeyHex), address, stagenet,
            )
        }.onFailure {
            DucatLog.e(TAG, "burned, but the proof could not be made: ${it.message}")
        }.getOrDefault("")

        val record = BurnRecord(
            personaHex = personaHex.lowercase(),
            txidHex = sent.txidHex.lowercase(),
            amountPxmr = amountPxmr,
            purpose = label,
            proof = proof,
            height = 0,
            envelopeHex = null,
            madeAt = System.currentTimeMillis() / 1000,
        )
        synchronized(lock) { putBurns(context, burns(context) + record) }
        DucatLog.i(
            TAG,
            "burned ${formatXmr(amountPxmr)} XMR under ${personaHex.take(8)}… ($label); " +
                "waiting for its block",
        )
        return record
    }

    /**
     * Give every burn that has reached a block its envelope. Run by the
     * poller's sweep, because the block arrives minutes after the send and
     * usually with the app in a pocket.
     *
     * Only the node in use is asked here — this is our own burn, and there is
     * nothing to defend against: a node that lied about the height would
     * produce an envelope the reader's own two nodes then refuse. The second
     * opinion belongs on the reading side, where the money is.
     */
    fun burnLap(context: Context) {
        val waiting = burns(context).filter { it.envelopeHex.isNullOrBlank() }
        if (waiting.isEmpty()) return
        val node = nodeInUse(context, probe = false) ?: return
        val roster = PersonaStore(context)
        val done = HashMap<String, Pair<Long, String>>()
        for (b in waiting) {
            if (b.proof.isBlank()) continue
            val status = runCatching {
                uniffi.ducat_mobile.moneroTxStatus(node, b.txidHex, NODE_TIMEOUT_MS)
            }.getOrNull() ?: continue
            val height = (status as? uniffi.ducat_mobile.TxStatus.InBlock)?.height?.toLong() ?: continue
            if (height <= 0) continue
            val secret = roster.secretFor(b.personaHex) ?: continue
            val envelope = runCatching {
                uniffi.ducat_mobile.burnProofSign(
                    uniffi.ducat_mobile.BurnProofIn(
                        personaSecret = secret,
                        txidHex = b.txidHex,
                        amountPxmr = b.amountPxmr.toULong(),
                        height = height.toULong(),
                        proof = b.proof,
                        purpose = b.purpose,
                    ),
                )
            }.onFailure { DucatLog.w(TAG, "burn proof: ${it.message}") }.getOrNull() ?: continue
            done[b.txidHex] = height to hex(envelope)
            DucatLog.i(
                TAG,
                "burn ${b.txidHex.take(12)}… is in block $height; its proof is ready to show",
            )
        }
        if (done.isEmpty()) return
        // Re-read inside the lock before writing: the sweep runs beside a
        // screen that may have started another burn while the node was
        // being asked, and a whole-list write from a stale snapshot would
        // drop it.
        synchronized(lock) {
            putBurns(
                context,
                burns(context).map { b ->
                    val got = done[b.txidHex]
                    if (got == null || !b.envelopeHex.isNullOrBlank()) b
                    else b.copy(height = got.first, envelopeHex = got.second)
                },
            )
        }
    }

    // ----- checking somebody else's --------------------------------------------

    /**
     * Check a stranger's burn proof the way §9.5 says, and keep the verdict.
     * [personaHex] is the persona presenting it — the message must name it.
     * Blocking; call it off the main thread.
     *
     * Every refusal throws with a sentence a screen can show, because the
     * difference between "this proof is for somebody else" and "it is not in
     * a block yet" is the difference between a fraud and a wait.
     */
    fun verifyBurn(context: Context, personaHex: String, envelopeHex: String): VerifiedBurn {
        val bytes = unhex(envelopeHex)
            ?: throw IllegalStateException(context.getString(R.string.burn_err_unreadable))
        val opened = runCatching { uniffi.ducat_mobile.burnProofOpen(bytes) }
            .getOrElse { throw IllegalStateException(context.getString(R.string.burn_err_unreadable)) }

        // 1. The persona presenting it is the persona the message names.
        if (!opened.personaHex.equals(personaHex.trim(), ignoreCase = true)) {
            throw IllegalStateException(context.getString(R.string.burn_err_other_persona))
        }
        val claimed = opened.amountPxmr.toLong()
        if (claimed < FLOOR_PXMR) {
            throw IllegalStateException(context.getString(R.string.burn_err_under_floor_theirs))
        }

        // 2. Our own node, for exactly the amount the proof proves.
        val node = nodeInUse(context, probe = true)
            ?: throw IllegalStateException(context.getString(R.string.burn_err_no_node))
        val stagenet = WalletStore(context).stagenet()
        val verified = runCatching {
            uniffi.ducat_mobile.moneroVerifyOutProof(
                node, opened.txidHex, uniffi.ducat_mobile.moneroBurnAddress(stagenet),
                stagenet, opened.message, opened.proof, NODE_TIMEOUT_MS,
            )
        }.getOrElse {
            DucatLog.w(TAG, "the node does not bear out ${opened.txidHex.take(12)}…: ${it.message}")
            throw IllegalStateException(context.getString(R.string.burn_err_node_refuses))
        }
        if (verified.amountPxmr.toLong() != claimed) {
            throw IllegalStateException(context.getString(R.string.burn_err_wrong_amount))
        }
        val height = verified.height.toLong()
        if (height <= 0) {
            throw IllegalStateException(context.getString(R.string.burn_err_no_block))
        }

        // 3. A second node has it in a block. Unknown, the pool and silence
        //    are all "not yet" — never "yes".
        if (!corroborated(context, opened.txidHex)) {
            throw IllegalStateException(context.getString(R.string.burn_err_no_second))
        }

        val verdict = VerifiedBurn(
            personaHex = opened.personaHex.lowercase(),
            txidHex = opened.txidHex.lowercase(),
            amountPxmr = claimed,
            height = height,
            purpose = opened.purpose,
            checkedAt = System.currentTimeMillis() / 1000,
        )
        synchronized(lock) {
            putVerified(
                context,
                verifiedBurns(context).filterNot {
                    it.personaHex.equals(verdict.personaHex, ignoreCase = true) &&
                        it.txidHex.equals(verdict.txidHex, ignoreCase = true)
                } + verdict,
            )
        }
        DucatLog.i(
            TAG,
            "verified a burn of ${formatXmr(claimed)} XMR by ${verdict.personaHex.take(8)}… " +
                "at block $height",
        )
        return verdict
    }

    /**
     * Does a node we are not already trusting have this transaction in a
     * block? The bridge picks which nodes those are — never the one in use,
     * never the operator's own — and both clients ask it the same way.
     */
    private fun corroborated(context: Context, txidHex: String): Boolean {
        val store = NodeStore(context)
        val others = runCatching {
            uniffi.ducat_mobile.moneroSecondOpinionNodes(
                store.lastGood()?.trim()?.ifBlank { null },
                store.ownUrl()?.trim()?.ifBlank { null },
            ).map { it.url }
        }.getOrDefault(emptyList())
        for (url in others) {
            val status = runCatching {
                uniffi.ducat_mobile.moneroTxStatus(url, txidHex, NODE_TIMEOUT_MS)
            }.getOrNull() ?: continue
            val height = (status as? uniffi.ducat_mobile.TxStatus.InBlock)?.height?.toLong() ?: continue
            if (height > 0) {
                DucatLog.i(TAG, "${txidHex.take(12)}… corroborated in block $height at $url")
                return true
            }
        }
        return false
    }

    // ----- the plumbing --------------------------------------------------------

    /**
     * The node this wallet is using, probing for one when the last good node
     * has been demoted. [probe] is false on the sweep — a lap with no node is
     * a lap that does nothing, and the poller has its own picker — and true
     * where somebody is waiting for an answer.
     */
    private fun nodeInUse(context: Context, probe: Boolean): String? {
        val store = NodeStore(context)
        store.lastGood()?.let { return it }
        if (!probe) return null
        return runCatching {
            uniffi.ducat_mobile.moneroPickNode(
                uniffi.ducat_mobile.moneroDefaultNodes(store.ownUrl()),
                "stagenet", 8_000u,
            ).also { store.rememberLastGood(it.url) }.url
        }.getOrNull()
    }

    /** A refusal in the reader's own language. */
    fun words(context: Context, refusal: Refusal): String = context.getString(
        when (refusal) {
            Refusal.UnderFloor -> R.string.burn_err_under_floor
            Refusal.NoPurpose -> R.string.burn_err_no_purpose
            Refusal.PurposeTooLong -> R.string.burn_err_purpose_long
        },
    )

    // ----- §9.2: rated receipts --------------------------------------------------
    //
    // A receipt is an `ATTESTATION` envelope: who spoke, who was spoken
    // about, how much settled between them, one to five stars, a time, and
    // at most one sentence. The bridge signs and opens it
    // (`mobile/src/attest.rs`) so that both clients seal the same bytes under
    // the same refusals; what a reader then concludes from one is this
    // object's arithmetic, and it is the desk's (`app/src/trust.rs`) rule
    // for rule.
    //
    // Three shelves. "given" is what this phone said about others; "received"
    // is what others said about this phone's personas, kept only when the
    // envelope was signed by the persona that sent it; "about" is what
    // strangers showed this phone about themselves. Only the third is ever
    // summed, and only here — a record is envelopes, never somebody else's
    // score, and every reader weighs them again (§9.5, "How a receipt
    // travels").

    private const val GIVEN = "given"
    private const val RECEIVED = "received"
    private const val ABOUT = "about"

    /** The link forms, exact and shared by both clients (§9.5, §9.2). */
    const val BURN_PREFIX = "ducat:burn/"
    const val ATTEST_PREFIX = "ducat:attest/"
    const val RECORD_PREFIX = "ducat:record/"
    const val VOUCH_PREFIX = "ducat:vouch/"
    const val VOUCHES_PREFIX = "ducat:vouches/"

    /** A record is at most this many envelopes; a reader stops there. */
    const val MAX_RECORD_ENVELOPES = 64

    /**
     * A record travels as one text message, and the wire caps a text at
     * `MAX_MESSAGE_CHARS` (core/src/contact.rs). Restated here so the link
     * is packed to fit before the bridge refuses it — sixty-four envelopes
     * are tens of kilobytes, and "sixty-four at most" is the reader's
     * ceiling, not a promise that they all fit one message.
     */
    const val MAX_LINK_CHARS = 2000

    const val RATING_MIN = 1
    const val RATING_MAX = 5

    /** One sentence — `MAX_ATTESTATION_NOTE_CHARS` in core/src/trust.rs. */
    const val MAX_NOTE_CHARS = 140

    /** §9.2 — a rated receipt, given, received, or read about someone. */
    data class AttestationRecord(
        /** Who spoke. */
        val signerHex: String,
        /** Who was spoken about. */
        val subjectHex: String,
        val amountPxmr: Long,
        val rating: Int,
        val ts: Long,
        val txidHex: String?,
        val note: String?,
        /** The signed envelope, hex, so it can be shown again. */
        val envelopeHex: String,
    )

    /**
     * What this phone can say about a persona's record: how many receipts
     * it has read, and how many distinct signers among them have a burn it
     * verified itself. One voice per signer — a burned persona that rates
     * the same subject ten times counts once, by its latest — so a record
     * cannot be padded by one friend with one burn.
     */
    data class RecordSummary(
        val receipts: Int = 0,
        val weighted: Int = 0,
        /** Mean rating over the weighted signers' latest receipts, times ten (47 = 4.7); zero when none. */
        val ratingX10: Int = 0,
    )

    /** A trust object riding a text body, recognised by its prefix. */
    sealed class Link {
        data class Burn(val hex: String) : Link()
        data class Attest(val hex: String) : Link()
        data class Record(val dotted: String) : Link()
        /** §9.2: one `VOUCH` envelope, sent by its signer to its subject. */
        data class Vouch(val hex: String) : Link()
        /** §9.2: the vouches a persona holds about itself, dotted, newest first. */
        data class Vouches(val dotted: String) : Link()
    }

    /**
     * The link a body carries, or null.
     *
     * The trimmed body must be the link and nothing else — the prefix at
     * the start, the payload to the end, no whitespace inside. That is the
     * rule the desk ingests by (`ingest_trust_links` strips the prefix from
     * the trimmed body and reads the rest as hex), kept here for drawing as
     * well, so the two clients never disagree about which messages were
     * receipts.
     */
    fun linkIn(body: String): Link? {
        val b = body.trim()
        val prefix = listOf(BURN_PREFIX, ATTEST_PREFIX, RECORD_PREFIX, VOUCH_PREFIX, VOUCHES_PREFIX)
            .firstOrNull { b.startsWith(it) } ?: return null
        val payload = b.substring(prefix.length)
        if (payload.isEmpty() || payload.any { it.isWhitespace() }) return null
        val hex = payload.all(::isHexChar)
        val dotted = payload.all { isHexChar(it) || it == '.' }
        return when (prefix) {
            BURN_PREFIX -> if (hex) Link.Burn(payload) else null
            ATTEST_PREFIX -> if (hex) Link.Attest(payload) else null
            VOUCH_PREFIX -> if (hex) Link.Vouch(payload) else null
            VOUCHES_PREFIX -> if (dotted) Link.Vouches(payload) else null
            else -> if (dotted) Link.Record(payload) else null
        }
    }

    private fun isHexChar(c: Char) = c in '0'..'9' || c in 'a'..'f' || c in 'A'..'F'

    /** The envelopes of a record, in the order shown, empties skipped, at most 64. */
    internal fun splitRecord(dotted: String): List<String> =
        dotted.split('.').filter { it.isNotEmpty() }.take(MAX_RECORD_ENVELOPES)

    /**
     * §9.2's arithmetic over what was read about one subject, pure so it can
     * be tested without a store. `receipts` is everything read; `weighted`
     * is the distinct signers among them that [burned] answers for; the
     * rating is the mean over those signers' *latest* receipts, times ten,
     * in integers — the desk's sum, the desk's division.
     */
    internal fun summarize(
        about: List<AttestationRecord>,
        burned: (signerHex: String) -> Boolean,
    ): RecordSummary {
        val latest = LinkedHashMap<String, AttestationRecord>()
        for (r in about) {
            val have = latest[r.signerHex]
            if (have == null || r.ts > have.ts) latest[r.signerHex] = r
        }
        var weighted = 0
        var sum = 0
        for ((signer, r) in latest) {
            if (!burned(signer)) continue
            weighted += 1
            sum += r.rating * 10
        }
        return RecordSummary(
            receipts = about.size,
            weighted = weighted,
            ratingX10 = if (weighted > 0) sum / weighted else 0,
        )
    }

    /**
     * The record a persona shows, packed to travel: the envelopes others
     * gave it, newest first, as many as fit one message and never more than
     * sixty-four. Null when there are none — nothing on the record yet.
     *
     * Newest first because a reader rates each signer by its latest
     * receipt, so when not everything fits, the envelopes that would change
     * the answer are the ones that go.
     */
    internal fun recordLinkOf(received: List<AttestationRecord>, personaHex: String): String? =
        packLink(
            RECORD_PREFIX,
            received
                .filter { it.subjectHex.equals(personaHex.trim(), ignoreCase = true) }
                .sortedByDescending { it.ts }
                .map { it.envelopeHex },
        )

    /**
     * Join envelopes, in the order given, into one link under [prefix]
     * that fits a message: as many as fit [MAX_LINK_CHARS], never more
     * than [MAX_RECORD_ENVELOPES]. Null when nothing was given. The desk's
     * `pack_link`, so a record and a set of vouches travel the same way.
     */
    internal fun packLink(prefix: String, envelopes: List<String>): String? {
        val out = StringBuilder(prefix)
        var n = 0
        for (env in envelopes) {
            if (n >= MAX_RECORD_ENVELOPES) break
            val add = (if (n == 0) 0 else 1) + env.length
            if (out.length + add > MAX_LINK_CHARS) break
            if (n > 0) out.append('.')
            out.append(env)
            n += 1
        }
        return if (n == 0) null else out.toString()
    }

    /**
     * The words a link body reads as in a list or a notification, or null
     * for an ordinary body. The bubble draws more (a button, a count); this
     * is the one line, so that a receipt never previews as a page of hex.
     */
    fun linkWords(context: Context, body: String, outgoing: Boolean): String? = when (linkIn(body)) {
        is Link.Burn -> context.getString(R.string.trust_burn_proof_msg)
        is Link.Attest -> context.getString(
            if (outgoing) R.string.trust_rating_sent else R.string.trust_rating_received,
        )
        is Link.Record -> context.getString(
            if (outgoing) R.string.trust_record_sent else R.string.trust_record_preview,
        )
        is Link.Vouch -> context.getString(
            if (outgoing) R.string.trust_vouch_sent else R.string.trust_vouch_received,
        )
        is Link.Vouches -> context.getString(
            if (outgoing) R.string.trust_vouches_sent else R.string.trust_vouches_preview,
        )
        null -> null
    }

    // ----- the shelves -----------------------------------------------------------
    //
    // JSON names are the ones both clients write — signer, subject, amount,
    // rating, ts, txid, note, envelope — so a bundle exported on either
    // restores on the other. txid and note are omitted rather than null.

    private fun readAttestations(context: Context, key: String): List<AttestationRecord> = runCatching {
        val arr = JSONArray(prefs(context).getString(key, "[]"))
        (0 until arr.length()).map {
            val o = arr.getJSONObject(it)
            AttestationRecord(
                signerHex = o.getString("signer"),
                subjectHex = o.getString("subject"),
                amountPxmr = o.getLong("amount"),
                rating = o.getInt("rating"),
                ts = o.getLong("ts"),
                txidHex = o.optString("txid", "").ifBlank { null },
                note = o.optString("note", "").ifBlank { null },
                envelopeHex = o.getString("envelope"),
            )
        }
    }.onFailure { DucatLog.w(TAG, "the $key receipts did not parse: ${it.message}") }
        .getOrDefault(emptyList())

    private fun putAttestations(context: Context, key: String, list: List<AttestationRecord>) {
        val arr = JSONArray()
        list.forEach { r ->
            arr.put(
                JSONObject().apply {
                    put("signer", r.signerHex)
                    put("subject", r.subjectHex)
                    put("amount", r.amountPxmr)
                    put("rating", r.rating)
                    put("ts", r.ts)
                    r.txidHex?.let { put("txid", it) }
                    r.note?.let { put("note", it) }
                    put("envelope", r.envelopeHex)
                },
            )
        }
        prefs(context).edit().putString(key, arr.toString()).apply()
        ContactStore.bump()
    }

    /** What others said about this phone's personas, each signed by its sender. */
    fun received(context: Context): List<AttestationRecord> = readAttestations(context, RECEIVED)

    /** What strangers showed this phone about themselves. */
    fun about(context: Context): List<AttestationRecord> = readAttestations(context, ABOUT)

    private fun recordFrom(v: uniffi.ducat_mobile.AttestationView, envelopeHex: String) = AttestationRecord(
        signerHex = v.signerHex.lowercase(),
        subjectHex = v.subjectHex.lowercase(),
        amountPxmr = v.amountPxmr.toLong(),
        rating = v.rating.toInt(),
        ts = v.ts.toLong(),
        txidHex = v.txidHex?.lowercase(),
        note = v.note,
        envelopeHex = envelopeHex.trim().lowercase(),
    )

    // ----- rating somebody -------------------------------------------------------

    /**
     * Rate [subjectHex] after a settled deal: sign an `ATTESTATION` under
     * [personaHex], keep it on the "given" shelf, and return it as the
     * `ducat:attest/` link the screen sends as an ordinary text.
     *
     * [personaHex] is the persona the thread speaks as — `ownerHexOf` the
     * contact, which is the worn persona on a phone with one — because the
     * reader keeps a receipt only when its signer is the message's sender.
     * Signed under any other name it would arrive, be opened, and be
     * dropped without a word.
     *
     * The refusals a stranger would make are made here first, in the
     * reader's language; the bridge makes them again before it seals.
     */
    fun attest(
        context: Context,
        personaHex: String,
        subjectHex: String,
        amountPxmr: Long,
        rating: Int,
        note: String?,
        txidHex: String?,
    ): String {
        if (rating !in RATING_MIN..RATING_MAX) {
            throw IllegalStateException(context.getString(R.string.trust_err_rating))
        }
        val sentence = note?.trim()?.ifBlank { null }
        if (sentence != null && sentence.codePointCount(0, sentence.length) > MAX_NOTE_CHARS) {
            throw IllegalStateException(context.getString(R.string.trust_err_note_long))
        }
        val subject = subjectHex.trim().lowercase()
        if (subject == personaHex.trim().lowercase()) {
            throw IllegalStateException(context.getString(R.string.trust_err_self))
        }
        val secret = PersonaStore(context).secretFor(personaHex)
            ?: throw IllegalStateException(context.getString(R.string.burn_err_no_persona))
        val envelope = runCatching {
            uniffi.ducat_mobile.attestationSign(
                uniffi.ducat_mobile.AttestationIn(
                    personaSecret = secret,
                    subjectHex = subject,
                    amountPxmr = amountPxmr.coerceAtLeast(0).toULong(),
                    rating = rating.toUByte(),
                    ts = (System.currentTimeMillis() / 1000).toULong(),
                    txidHex = txidHex?.trim()?.ifBlank { null },
                    note = sentence,
                ),
            )
        }.onFailure { DucatLog.w(TAG, "attestation: ${it.message}") }
            .getOrElse { throw IllegalStateException(context.getString(R.string.trust_err_unsigned)) }
        // Read back through the wire's own reader, so the record on the
        // shelf is what the envelope says and not what this side meant.
        val opened = runCatching { uniffi.ducat_mobile.attestationOpen(envelope) }
            .getOrElse { throw IllegalStateException(context.getString(R.string.trust_err_unsigned)) }
        val envelopeHex = hex(envelope)
        val rec = recordFrom(opened, envelopeHex)
        synchronized(lock) { putAttestations(context, GIVEN, readAttestations(context, GIVEN) + rec) }
        DucatLog.i(
            TAG,
            "rated ${subject.take(8)}…: $rating star(s) on ${formatXmr(amountPxmr)} XMR",
        )
        return ATTEST_PREFIX + envelopeHex
    }

    // ----- what arrives ----------------------------------------------------------

    /**
     * A receipt about one of this phone's personas, from the thread. Kept
     * only if the envelope opens under the persona that sent it — a receipt
     * handed on by anyone but its signer is discarded — and only if it is
     * about one of our own names. Deduped on (signer, ts): the same envelope
     * twice is one receipt. Null when dropped, with the reason in the log;
     * nothing on a screen waits for this.
     */
    fun receiveAttestation(context: Context, fromHex: String, envelopeHex: String): AttestationRecord? {
        val bytes = unhex(envelopeHex) ?: return dropped(fromHex, "not hex")
        val v = runCatching { uniffi.ducat_mobile.attestationOpen(bytes) }
            .getOrElse { return dropped(fromHex, "does not open: ${it.message}") }
        if (!v.signerHex.equals(fromHex.trim(), ignoreCase = true)) {
            return dropped(fromHex, "not signed by the sender")
        }
        val mine = PersonaStore(context).allHexes().map { it.lowercase() }
        if (v.subjectHex.lowercase() !in mine) return dropped(fromHex, "not about us")
        val rec = recordFrom(v, envelopeHex)
        synchronized(lock) {
            putAttestations(
                context, RECEIVED,
                readAttestations(context, RECEIVED)
                    .filterNot { it.signerHex == rec.signerHex && it.ts == rec.ts } + rec,
            )
        }
        DucatLog.i(TAG, "a receipt from ${fromHex.take(8)}…: ${rec.rating} star(s)")
        return rec
    }

    private fun dropped(fromHex: String, why: String): AttestationRecord? {
        DucatLog.w(TAG, "a receipt from ${fromHex.take(8)}… was dropped: $why")
        return null
    }

    /**
     * The receipts others gave [personaHex], as the `ducat:record/` link to
     * send when somebody asks for the record; null when there are none.
     */
    fun myRecordLink(context: Context, personaHex: String): String? =
        recordLinkOf(received(context), personaHex)

    /**
     * A record somebody showed us: each envelope that opens and is about the
     * sender is kept on the "about" shelf, deduped on (signer, subject, ts);
     * the rest are dropped without comment. Returns what we can now say
     * about the sender.
     */
    fun readRecord(context: Context, fromHex: String, dotted: String): RecordSummary {
        val from = fromHex.trim()
        var taken = 0
        synchronized(lock) {
            val about = readAttestations(context, ABOUT).toMutableList()
            for (h in splitRecord(dotted)) {
                val bytes = unhex(h) ?: continue
                val v = runCatching { uniffi.ducat_mobile.attestationOpen(bytes) }.getOrNull() ?: continue
                if (!v.subjectHex.equals(from, ignoreCase = true)) continue
                val rec = recordFrom(v, h)
                about.removeAll {
                    it.signerHex == rec.signerHex && it.subjectHex == rec.subjectHex && it.ts == rec.ts
                }
                about.add(rec)
                taken += 1
            }
            if (taken > 0) putAttestations(context, ABOUT, about)
        }
        DucatLog.i(TAG, "${from.take(8)}… showed a record: $taken receipt(s) read")
        return recordOf(context, from)
    }

    /**
     * What we hold about a persona's record, weighted by the signers whose
     * burns we verified ourselves (§9.2). Zero across the board for a
     * stranger who has shown nothing.
     */
    fun recordOf(context: Context, personaHex: String): RecordSummary {
        val subject = personaHex.trim()
        val about = about(context).filter { it.subjectHex.equals(subject, ignoreCase = true) }
        if (about.isEmpty()) return RecordSummary()
        val burned = verifiedBurns(context).mapTo(HashSet()) { it.personaHex.lowercase() }
        return summarize(about) { it.lowercase() in burned }
    }

    /**
     * Called for every incoming text, from the one place the thread appends
     * one. A `ducat:attest/` link is a receipt about one of our personas
     * from the sender; a `ducat:record/` link is the sender showing what
     * others said about them; a `ducat:vouch/` link is the sender saying
     * they know one of our personas; a `ducat:vouches/` link is the sender
     * showing who knows them. Each is opened under the signer it names and
     * refused unless the sender is who the object says it is. A burn proof
     * is not ingested: it is checked when somebody presses the button,
     * because the check costs two node round trips. Never throws — the
     * message is already taken, and an envelope that will not open is the
     * sender's problem, not the thread's.
     */
    fun ingestTrustLinks(context: Context, fromHex: String, body: String) {
        runCatching {
            when (val link = linkIn(body)) {
                is Link.Attest -> receiveAttestation(context, fromHex, link.hex)
                is Link.Record -> readRecord(context, fromHex, link.dotted)
                is Link.Vouch -> receiveVouch(context, fromHex, link.hex)
                is Link.Vouches -> readVouches(context, fromHex, link.dotted)
                else -> null
            }
        }.onFailure { DucatLog.w(TAG, "trust link from ${fromHex.take(8)}…: ${it.message}") }
    }

    // ----- §9.2: vouching --------------------------------------------------------
    //
    // A vouch is the smallest signed thing in the protocol: signer, subject,
    // a time — *I know this persona*, said by someone who met them, and
    // nothing else, because the less it carries the less it leaks. The
    // bridge seals and opens it (`mobile/src/attest.rs`) under the wire's
    // refusals — nowhen, oneself, a signature under any key but the one
    // named inside — so both clients agree about what a vouch is.
    //
    // It travels as a receipt does. The signer sends it to its subject as a
    // `ducat:vouch/` link, kept only when sent by its signer and about one
    // of this phone's personas; a persona shows what it holds as
    // `ducat:vouches/`, newest first, as many as fit one message. What a
    // reader then does with it is arithmetic over its *own* contacts: a
    // vouch counts only when its signer is a contact this phone already
    // holds, and the answer is worn in words — never a score, never
    // published, never forwarded. A friend of a friend is a stranger with a
    // story: nothing past one hop is computed.
    //
    // Three shelves, as for receipts. The desk's `app/src/trust.rs`, rule
    // for rule.

    private const val VOUCHES_GIVEN = "vouches_given"
    private const val VOUCHES_RECEIVED = "vouches_received"
    private const val VOUCHES_ABOUT = "vouches_about"

    /** §9.2 — a vouch, given, received, or read about someone. */
    data class VouchRecord(
        /** Who says they know the subject; the envelope's signer. */
        val signerHex: String,
        /** Who is known. */
        val subjectHex: String,
        val ts: Long,
        /** The signed envelope, hex, so it can be shown again. */
        val envelopeHex: String,
    )

    // JSON names are the ones both clients write — signer, subject, ts,
    // envelope — so a bundle exported on either restores on the other.

    private fun vouchShelf(context: Context, key: String): List<VouchRecord> = runCatching {
        val arr = JSONArray(prefs(context).getString(key, "[]"))
        (0 until arr.length()).map {
            val o = arr.getJSONObject(it)
            VouchRecord(
                signerHex = o.getString("signer"),
                subjectHex = o.getString("subject"),
                ts = o.getLong("ts"),
                envelopeHex = o.getString("envelope"),
            )
        }
    }.onFailure { DucatLog.w(TAG, "the $key shelf did not parse: ${it.message}") }
        .getOrDefault(emptyList())

    private fun putVouchShelf(context: Context, key: String, list: List<VouchRecord>) {
        val arr = JSONArray()
        list.forEach { r ->
            arr.put(
                JSONObject().apply {
                    put("signer", r.signerHex)
                    put("subject", r.subjectHex)
                    put("ts", r.ts)
                    put("envelope", r.envelopeHex)
                },
            )
        }
        prefs(context).edit().putString(key, arr.toString()).apply()
        ContactStore.bump()
    }

    private fun vouchFrom(v: uniffi.ducat_mobile.VouchView, envelopeHex: String) = VouchRecord(
        signerHex = v.signerHex.lowercase(),
        subjectHex = v.subjectHex.lowercase(),
        ts = v.ts.toLong(),
        envelopeHex = envelopeHex.trim().lowercase(),
    )

    // The rules, over lists, testable without a store.

    /** A persona vouching for itself — refused before anything is signed. */
    internal fun selfVouch(signerHex: String, subjectHex: String): Boolean =
        signerHex.trim().equals(subjectHex.trim(), ignoreCase = true)

    /**
     * One vouch per (signer, subject): [rec] replaces whatever the same
     * signer said about the same subject before. A vouch says one thing,
     * so a second from the same mouth is the same thing said again, and
     * its newer time is all that changes.
     */
    internal fun withVouch(list: List<VouchRecord>, rec: VouchRecord): List<VouchRecord> =
        list.filterNot {
            it.signerHex.equals(rec.signerHex, ignoreCase = true) &&
                it.subjectHex.equals(rec.subjectHex, ignoreCase = true)
        } + rec

    /**
     * One vouch per subject on the "given" shelf, whichever of this phone's
     * personas signed it: vouching again for the same person is the same
     * vouch, re-signed.
     */
    internal fun givenWith(given: List<VouchRecord>, rec: VouchRecord): List<VouchRecord> =
        given.filterNot { it.subjectHex.equals(rec.subjectHex, ignoreCase = true) } + rec

    /**
     * The vouches a persona shows, packed to travel: the ones others gave
     * it, newest first, as many as fit one message and never more than
     * sixty-four. Null when there are none — nobody has vouched yet.
     */
    internal fun vouchesLinkOf(received: List<VouchRecord>, personaHex: String): String? =
        packLink(
            VOUCHES_PREFIX,
            received
                .filter { it.subjectHex.equals(personaHex.trim(), ignoreCase = true) }
                .sortedByDescending { it.ts }
                .map { it.envelopeHex },
        )

    /**
     * §9.2's arithmetic: who among the reader's own contacts vouched for
     * [personaHex], as names, from what was read about them. [mine] is
     * this phone's own personas — our own vouch is not a contact's — and
     * [nameOf] answers for a signer only when it is a contact this phone
     * holds. Sorted and distinct, so two rows from one signer are one name.
     */
    internal fun knownByOf(
        about: List<VouchRecord>,
        personaHex: String,
        mine: Set<String>,
        nameOf: (signerHex: String) -> String?,
    ): List<String> {
        val subject = personaHex.trim().lowercase()
        val ours = mine.mapTo(HashSet()) { it.lowercase() }
        return about.asSequence()
            .filter { it.subjectHex.lowercase() == subject }
            .map { it.signerHex.lowercase() }
            .filter { it !in ours }
            .mapNotNull(nameOf)
            .distinct()
            .sorted()
            .toList()
    }

    /** Whether any of this phone's personas vouched for [subjectHex]. */
    fun vouchedFor(context: Context, subjectHex: String): Boolean {
        val subject = subjectHex.trim()
        return vouchShelf(context, VOUCHES_GIVEN).any { it.subjectHex.equals(subject, ignoreCase = true) }
    }

    /**
     * Vouch for [subjectHex] — *I know this persona* — under [personaHex],
     * keep it on the "given" shelf, and return it as the `ducat:vouch/`
     * link the screen sends as an ordinary text. There is nothing to type:
     * a vouch says one thing.
     *
     * [personaHex] is the persona the thread speaks as (`ownerHexOf` the
     * contact), because the reader keeps a vouch only when its signer is
     * the message's sender. One's own name is refused before signing; the
     * bridge refuses it again before it seals.
     */
    fun vouch(context: Context, personaHex: String, subjectHex: String): String {
        val subject = subjectHex.trim().lowercase()
        if (selfVouch(personaHex, subject)) {
            throw IllegalStateException(context.getString(R.string.trust_err_vouch_self))
        }
        val secret = PersonaStore(context).secretFor(personaHex)
            ?: throw IllegalStateException(context.getString(R.string.burn_err_no_persona))
        val envelope = runCatching {
            uniffi.ducat_mobile.vouchSign(
                uniffi.ducat_mobile.VouchIn(
                    personaSecret = secret,
                    subjectHex = subject,
                    ts = (System.currentTimeMillis() / 1000).toULong(),
                ),
            )
        }.onFailure { DucatLog.w(TAG, "vouch: ${it.message}") }
            .getOrElse { throw IllegalStateException(context.getString(R.string.trust_err_vouch_unsigned)) }
        // Read back through the wire's own reader, so the shelf holds what
        // the envelope says and not what this side meant.
        val opened = runCatching { uniffi.ducat_mobile.vouchOpen(envelope) }
            .getOrElse { throw IllegalStateException(context.getString(R.string.trust_err_vouch_unsigned)) }
        val envelopeHex = hex(envelope)
        val rec = vouchFrom(opened, envelopeHex)
        synchronized(lock) {
            putVouchShelf(context, VOUCHES_GIVEN, givenWith(vouchShelf(context, VOUCHES_GIVEN), rec))
        }
        DucatLog.i(TAG, "vouched for ${subject.take(8)}…")
        return VOUCH_PREFIX + envelopeHex
    }

    /**
     * A vouch about one of this phone's personas, from the thread. Kept
     * only if the envelope opens under the persona that sent it — a vouch
     * handed on by anyone but its signer is discarded — and only if it is
     * about one of our own names. One per (signer, subject). Null when
     * dropped, with the reason in the log; nothing on a screen waits for
     * this.
     */
    fun receiveVouch(context: Context, fromHex: String, envelopeHex: String): VouchRecord? {
        val bytes = unhex(envelopeHex) ?: return droppedVouch(fromHex, "not hex")
        val v = runCatching { uniffi.ducat_mobile.vouchOpen(bytes) }
            .getOrElse { return droppedVouch(fromHex, "does not open: ${it.message}") }
        if (!v.signerHex.equals(fromHex.trim(), ignoreCase = true)) {
            return droppedVouch(fromHex, "not signed by the sender")
        }
        val mine = PersonaStore(context).allHexes().map { it.lowercase() }
        if (v.subjectHex.lowercase() !in mine) return droppedVouch(fromHex, "not about us")
        val rec = vouchFrom(v, envelopeHex)
        synchronized(lock) {
            putVouchShelf(context, VOUCHES_RECEIVED, withVouch(vouchShelf(context, VOUCHES_RECEIVED), rec))
        }
        DucatLog.i(TAG, "${fromHex.take(8)}… vouched for us")
        return rec
    }

    private fun droppedVouch(fromHex: String, why: String): VouchRecord? {
        DucatLog.w(TAG, "a vouch from ${fromHex.take(8)}… was dropped: $why")
        return null
    }

    /**
     * The vouches others gave [personaHex], as the one `ducat:vouches/`
     * message to send when somebody should see who knows us; null when
     * nobody has vouched for this persona yet.
     */
    fun myVouchesLink(context: Context, personaHex: String): String? =
        vouchesLinkOf(vouchShelf(context, VOUCHES_RECEIVED), personaHex)

    /**
     * Vouches somebody showed us about themselves: each envelope that
     * opens and is about the sender is kept on the "about" shelf, one per
     * (signer, subject); the rest are dropped without comment. Returns who
     * among our contacts now vouches for the sender.
     */
    fun readVouches(context: Context, fromHex: String, dotted: String): List<String> {
        val from = fromHex.trim()
        var taken = 0
        synchronized(lock) {
            var about = vouchShelf(context, VOUCHES_ABOUT)
            for (h in splitRecord(dotted)) {
                val bytes = unhex(h) ?: continue
                val v = runCatching { uniffi.ducat_mobile.vouchOpen(bytes) }.getOrNull() ?: continue
                if (!v.subjectHex.equals(from, ignoreCase = true)) continue
                about = withVouch(about, vouchFrom(v, h))
                taken += 1
            }
            if (taken > 0) putVouchShelf(context, VOUCHES_ABOUT, about)
        }
        DucatLog.i(TAG, "${from.take(8)}… showed $taken vouch(es)")
        return knownBy(context, from)
    }

    /**
     * Who among *this phone's* contacts vouched for [personaHex] — display
     * names, for "Pat and Sam know them". Computed here, from what this
     * phone holds, and never sent anywhere; our own vouch is not a
     * contact's, and nothing past one hop is asked.
     */
    fun knownBy(context: Context, personaHex: String): List<String> {
        val about = vouchShelf(context, VOUCHES_ABOUT)
        if (about.none { it.subjectHex.equals(personaHex.trim(), ignoreCase = true) }) return emptyList()
        val mine = PersonaStore(context).allHexes().mapTo(HashSet()) { it.lowercase() }
        val names = HashMap<String, String>()
        for (c in ContactStore(context).all()) names[c.personaHex.lowercase()] = c.displayName()
        return knownByOf(about, personaHex, mine) { names[it] }
    }

    // ----- the backup ------------------------------------------------------------
    //
    // Eight shelves ride the bundle as the JSON text they are kept in, under
    // names both clients write. A burn is money already destroyed and its
    // envelope is the only thing that says so; a restored phone without it
    // has paid for nothing.

    private val BACKUP = linkedMapOf(
        "burns_raw" to OURS,
        "verified_burns_raw" to THEIRS,
        "attestations_given_raw" to GIVEN,
        "attestations_received_raw" to RECEIVED,
        "attestations_about_raw" to ABOUT,
        "vouches_given_raw" to VOUCHES_GIVEN,
        "vouches_received_raw" to VOUCHES_RECEIVED,
        "vouches_about_raw" to VOUCHES_ABOUT,
    )

    /** The bundle's names for the shelves, in the order they are written. */
    val BACKUP_KEYS: List<String> get() = BACKUP.keys.toList()

    /** Each shelf that has anything on it, as (bundle name, JSON text). */
    fun backupEntries(context: Context): Map<String, String> {
        val p = prefs(context)
        return BACKUP.entries.mapNotNull { (name, key) -> p.getString(key, null)?.let { name to it } }.toMap()
    }

    /**
     * Restore one shelf from the bundle: the whole list replaced under the
     * lock. A name this build does not know, or text that is not a JSON
     * list, leaves the shelf alone — a bundle must never be able to empty
     * a store it could not fill.
     */
    fun restoreEntry(context: Context, name: String, json: String): Boolean {
        val key = BACKUP[name] ?: return false
        if (json.isBlank()) return false
        if (runCatching { JSONArray(json) }.isFailure) {
            DucatLog.w(TAG, "the bundle's $name is not a list; left alone")
            return false
        }
        synchronized(lock) { prefs(context).edit().putString(key, json).apply() }
        ContactStore.bump()
        return true
    }
}
