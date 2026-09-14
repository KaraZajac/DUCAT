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
}
