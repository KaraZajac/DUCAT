package org.ducatproject.ducat

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/**
 * The bond ceremony's orchestration (§17.9): the glue between the sealed
 * threads and the threshold engine in the bridge.
 *
 * The crypto lives in Rust (`ceremony.rs`, machines held by ceremony id);
 * this drives it over DUCAT's pairwise threads. A ceremony has a roster of
 * two or three personas — two principals, optionally an arbiter — and a
 * threshold of two. Every round-0 message carries the roster, because a
 * pairwise thread only names two of the parties and the third has to learn
 * who else is in the room from the invitation itself. Everyone must be
 * mutual contacts of everyone: the shares travel the threads, and a thread
 * that does not exist cannot carry one.
 *
 * With three parties the deposit stops being strandable: any two shares can
 * sign, so a lost phone loses nothing and a dispute has somewhere to go.
 * The arbiter holds a share and nothing else — it cannot move money alone,
 * and unless it is called on, it never has to do anything after the build.
 *
 * State survives process death in prefs, because a ceremony spans poll
 * cycles and app restarts; the in-memory engine machines do not survive, so
 * a restart mid-ceremony aborts cleanly (the peers time out and retry) —
 * "nothing happens" is not left as a silent default (§9.3.4).
 */
object Ceremony {
    private const val TAG = "DucatCeremony"

    /** Threshold: always two signatures, whatever the roster size. */
    private const val T = 2

    private fun prefs(context: Context) =
        securePrefs(context, "ducat_ceremonies")

    private fun load(context: Context, id: String): JSONObject? =
        prefs(context).getString("c_$id", null)?.let { JSONObject(it) }

    private fun save(context: Context, id: String, o: JSONObject) =
        prefs(context).edit().putString("c_$id", o.toString()).apply()

    /** Every ceremony this device knows, for the UI to show. */
    fun all(context: Context): List<JSONObject> =
        prefs(context).all.keys
            .filter { it.startsWith("c_") }
            .mapNotNull { k -> prefs(context).getString(k, null)?.let { JSONObject(it) } }

    /**
     * The 32-byte context every party derives identically: the sorted
     * personas and a nonce, so a fresh bond never collides with an old one.
     */
    private fun ceremonyId(roster: List<String>, nonce: String): ByteArray {
        val md = java.security.MessageDigest.getInstance("SHA-256")
        md.update("DUCAT-BOND-v0".toByteArray())
        roster.forEach { md.update(it.toByteArray()) }
        md.update(nonce.toByteArray())
        return md.digest()
    }

    /** 1-based index of a persona in the sorted roster — the participant id. */
    private fun indexOf(roster: List<String>, personaHex: String): Int =
        roster.indexOf(personaHex) + 1

    // ===== Round-0 framing =====
    //
    // [u8 n][n × 32-byte personas, sorted][u8 arbiterIdx (0 = none)]
    // [u8 kind (0 = bond, 1 = ride)][u8 funderIdx (0 = none)]
    // [u64 farePxmr LE][u8 nonceLen][nonce][commitment…]
    //
    // The same format for two parties and three: one parser, no special
    // cases, and a 2-party bond is simply a roster with no arbiter. The kind
    // rides the invitation because the joiner's behaviour depends on it — a
    // ride's release needs a human yes, a bond's return does not — and the
    // fare rides it so the escrow *names its own amount*: the driver checks
    // it against the fare the accept echoed, not against a separate message.

    /** A deposit between two parties; release auto-cosigns (the proven flow). */
    const val KIND_BOND = 0
    /** A fare in escrow (§15.12): funder pays in, payee proposes the release,
     *  and the funder's yes is a screen, never an automatic signature. */
    const val KIND_RIDE = 1

    private fun frameRound0(
        roster: List<String>,
        arbiterIdx: Int,
        kind: Int,
        funderIdx: Int,
        farePxmr: Long,
        nonce: String,
        commitment: ByteArray,
    ): ByteArray {
        val out = java.io.ByteArrayOutputStream()
        out.write(roster.size)
        roster.forEach { out.write(hexToBytes(it)!!) }
        out.write(arbiterIdx)
        out.write(kind)
        out.write(funderIdx)
        out.write(
            java.nio.ByteBuffer.allocate(8)
                .order(java.nio.ByteOrder.LITTLE_ENDIAN).putLong(farePxmr).array()
        )
        val nb = nonce.toByteArray()
        out.write(nb.size)
        out.write(nb)
        out.write(commitment)
        return out.toByteArray()
    }

    private data class Invite(
        val roster: List<String>,
        val arbiterIdx: Int,
        val kind: Int,
        val funderIdx: Int,
        val farePxmr: Long,
        val nonce: String,
        val commitment: ByteArray,
    )

    private fun parseRound0(payload: ByteArray): Invite? {
        var p = 0
        fun take(n: Int): ByteArray? {
            if (p + n > payload.size) return null
            return payload.copyOfRange(p, p + n).also { p += n }
        }
        val n = take(1)?.get(0)?.toInt() ?: return null
        if (n < 2 || n > 3) return null
        val roster = (0 until n).map { take(32)?.toHexString() ?: return null }
        val arbiterIdx = take(1)?.get(0)?.toInt() ?: return null
        val kind = take(1)?.get(0)?.toInt() ?: return null
        val funderIdx = take(1)?.get(0)?.toInt() ?: return null
        val fare = take(8)?.let {
            java.nio.ByteBuffer.wrap(it).order(java.nio.ByteOrder.LITTLE_ENDIAN).long
        } ?: return null
        val nonceLen = take(1)?.get(0)?.toInt() ?: return null
        val nonce = String(take(nonceLen) ?: return null)
        val commitment = payload.copyOfRange(p, payload.size)
        if (commitment.isEmpty()) return null
        return Invite(roster, arbiterIdx, kind, funderIdx, fare, nonce, commitment)
    }

    private fun contactFor(context: Context, personaHex: String): Contact? =
        ContactStore(context).all().firstOrNull { it.personaHex == personaHex }

    /**
     * Start a bond with a contact — and optionally an arbiter, who must be a
     * mutual contact of both sides (the shares travel the threads; missing
     * threads mean an impossible ceremony, discovered by whoever lacks one).
     */
    fun startBond(context: Context, contact: Contact, arbiter: Contact? = null): String =
        start(context, contact, arbiter, KIND_BOND, 0L)

    /**
     * Start a ride escrow (§15.12): the caller is the rider — the funder —
     * the contact is the driver, and the fare is the number the accept
     * echoed. 2-of-3 with the arbiter, so a lost phone strands nothing and
     * a dispute has somewhere to go; the arbiter holds a share and, on the
     * happy path, never hears from anyone again.
     */
    fun startRide(context: Context, driver: Contact, arbiter: Contact, farePxmr: Long): String =
        start(context, driver, arbiter, KIND_RIDE, farePxmr)

    private fun start(
        context: Context,
        contact: Contact,
        arbiter: Contact?,
        kind: Int,
        farePxmr: Long,
    ): String {
        val mineHex = PersonaStore(context).personaHex()
        val roster = buildList {
            add(mineHex); add(contact.personaHex); arbiter?.let { add(it.personaHex) }
        }.sorted()
        val arbiterIdx = arbiter?.let { indexOf(roster, it.personaHex) } ?: 0
        val funderIdx = indexOf(roster, mineHex)
        val nonce = java.util.UUID.randomUUID().toString().take(8)
        val id = ceremonyId(roster, nonce)
        val idHex = id.toHexString()
        val i = funderIdx
        val n = roster.size

        val commit = uniffi.ducat_mobile.dkgCommit(id, i.toUShort(), T.toUShort(), n.toUShort())
        val frame = frameRound0(roster, arbiterIdx, kind, funderIdx, farePxmr, nonce, commit)
        for (peerHex in roster.filter { it != mineHex }) {
            val peer = contactFor(context, peerHex)
                ?: throw IllegalStateException("everyone in a bond must be your contact")
            Mailbox.send(
                context, peer, "bond: building a shared deposit",
                mineHex, kind = 8, round = 0, ceremonyId = id, payload = frame,
            )
        }
        val o = JSONObject().apply {
            put("id", idHex); put("nonce", nonce)
            put("roster", JSONArray(roster)); put("arbiterIdx", arbiterIdx)
            put("kind", kind); put("funderIdx", funderIdx); put("farePxmr", farePxmr)
            put("created", System.currentTimeMillis())
            put("peer", contact.personaHex)
            put("i", i); put("stage", "committed")
            put("commits", JSONObject()); put("shares", JSONObject())
        }
        save(context, idHex, o)
        DucatLog.i(TAG, "started ${if (kind == KIND_RIDE) "ride escrow" else "bond"} $idHex (i=$i of $n)")
        return idHex
    }

    /**
     * A DkgRound arrived. Record it, and advance the engine when everything
     * it was waiting for is in. Out-of-stage rounds are ignored (§2.5).
     *
     * @Synchronized because several loops poll the mailbox at once — the
     * global poller, a screen's pump, the hail's wait — and two of them
     * dispatching round-0s concurrently each saw "no ceremony yet", joined
     * twice, and double-committed: two engine machines for one id, and two
     * rapid sends racing the same outbox ring (found live, first bonded-ride
     * run 2026-08-16 — the share the ring lost stranded the escrow).
     */
    @Synchronized
    fun onDkgRound(
        context: Context,
        contact: Contact,
        ceremonyId: ByteArray?,
        round: Long?,
        payload: ByteArray?,
    ) {
        val id = ceremonyId ?: return
        val idHex = id.toHexString()
        payload ?: return
        round ?: return
        val mineHex = PersonaStore(context).personaHex()

        var o = load(context, idHex)

        // A round-0 with no ceremony of ours is an invitation: learn the
        // roster from the frame, verify the id really is that roster, join
        // by committing to everyone, then record the sender's commitment.
        if (o == null && round.toInt() == 0) {
            val inv = parseRound0(payload) ?: return
            if (mineHex !in inv.roster) return
            if (!ceremonyId(inv.roster, inv.nonce).contentEquals(id)) {
                DucatLog.w(TAG, "bond $idHex: roster does not hash to the ceremony id")
                return
            }
            val i = indexOf(inv.roster, mineHex)
            val n = inv.roster.size
            val commit =
                uniffi.ducat_mobile.dkgCommit(id, i.toUShort(), T.toUShort(), n.toUShort())
            // The join echoes the invite's own kind/funder/fare — the frame
            // is the ceremony's self-description and every copy must agree.
            val frame = frameRound0(
                inv.roster, inv.arbiterIdx, inv.kind, inv.funderIdx, inv.farePxmr,
                inv.nonce, commit,
            )
            for (peerHex in inv.roster.filter { it != mineHex }) {
                val peer = contactFor(context, peerHex) ?: run {
                    DucatLog.w(TAG, "bond $idHex: ${peerHex.take(8)}… is not my contact — cannot join")
                    return
                }
                Mailbox.send(
                    context, peer, "bond: building a shared deposit",
                    mineHex, kind = 8, round = 0, ceremonyId = id, payload = frame,
                )
            }
            o = JSONObject().apply {
                put("id", idHex); put("nonce", inv.nonce)
                put("roster", JSONArray(inv.roster)); put("arbiterIdx", inv.arbiterIdx)
                put("kind", inv.kind); put("funderIdx", inv.funderIdx)
                put("farePxmr", inv.farePxmr)
                put("created", System.currentTimeMillis())
                put("peer", contact.personaHex)
                put("i", i); put("stage", "committed")
                put("commits", JSONObject()); put("shares", JSONObject())
            }
            save(context, idHex, o)
            DucatLog.i(TAG, "joined bond $idHex (i=$i of $n), sent commitment")
        }
        o ?: return

        val roster = o.optJSONArray("roster")?.let { arr ->
            (0 until arr.length()).map { arr.getString(it) }
        } ?: return
        val n = roster.size
        val i = o.optInt("i")
        val senderIdx = indexOf(roster, contact.personaHex)
        if (senderIdx == 0) return

        // Record what arrived into its bucket, whatever stage we are in — a
        // peer that finished collecting commitments before us sends its share
        // early, and in a three-party bond that is the common case, not the
        // exception. Dropping an early share left one party stuck at "shared"
        // forever with n-2 shares (found live, first 2-of-3 run 2026-08-17).
        when (round.toInt()) {
            0 -> parseRound0(payload)?.let {
                o.getJSONObject("commits").put(senderIdx.toString(), it.commitment.toHexString())
            }
            1 -> o.getJSONObject("shares").put(senderIdx.toString(), payload.toHexString())
        }
        save(context, idHex, o)

        // Then advance through every stage the collected material now allows,
        // in one pass: commitments-complete → send shares; shares-complete →
        // finish. A single poll can carry both transitions.
        runCatching {
            val commits = o.getJSONObject("commits")
            if (o.optString("stage") == "committed" && commits.length() >= n - 1) {
                val from = commits.keys().asSequence().map { k ->
                    uniffi.ducat_mobile.FromParty(
                        k.toInt().toUShort(), hexToBytes(commits.getString(k))!!,
                    )
                }.toList()
                val shares = uniffi.ducat_mobile.dkgShare(
                    id, i.toUShort(), T.toUShort(), n.toUShort(), from,
                )
                for (s in shares) {
                    val peerHex = roster[s.participant.toInt() - 1]
                    val peer = contactFor(context, peerHex) ?: continue
                    Mailbox.send(
                        context, peer, "bond: your share",
                        mineHex, kind = 8, round = 1, ceremonyId = id, payload = s.bytes,
                    )
                }
                o.put("stage", "shared"); save(context, idHex, o)
                DucatLog.i(TAG, "bond $idHex: shared, sent ${shares.size} share(s)")
            } else if (o.optString("stage") == "committed") {
                DucatLog.i(TAG, "bond $idHex: commitment ${commits.length()}/${n - 1}")
            }

            val sh = o.getJSONObject("shares")
            if (o.optString("stage") == "shared" && sh.length() >= n - 1) {
                val from = sh.keys().asSequence().map { k ->
                    uniffi.ducat_mobile.FromParty(
                        k.toInt().toUShort(), hexToBytes(sh.getString(k))!!,
                    )
                }.toList()
                val addr = uniffi.ducat_mobile.dkgFinish(
                    id, i.toUShort(), T.toUShort(), n.toUShort(), from, true,
                )
                val keys = uniffi.ducat_mobile.dkgTakeKeys(id, i.toUShort())
                o.put("stage", "done"); o.put("address", addr)
                o.put("keys", keys.toHexString())
                // Where this device's escrow scans start: the chain as of the
                // build, minus a safety margin. The escrow was minted seconds
                // ago — its funding needs minutes of chain, not the wallet's
                // whole history (frost_propose gets the same number later).
                runCatching {
                    val h = uniffi.ducat_mobile.moneroPickNode(
                        uniffi.ducat_mobile.moneroDefaultNodes(NodeStore(context).ownUrl()),
                        "stagenet", 8_000u,
                    ).height.toLong()
                    o.put("scanFrom", (h - 10).coerceAtLeast(0))
                }
                save(context, idHex, o)
                ContactStore.bump()
                DucatLog.i(TAG, "bond $idHex done — escrow $addr")
            } else if (o.optString("stage") == "shared") {
                DucatLog.i(TAG, "bond $idHex: share ${sh.length()}/${n - 1}")
            }
        }.onFailure {
            DucatLog.w(TAG, "bond $idHex round $round failed: ${it.message}")
            uniffi.ducat_mobile.ceremonyAbort(id, i.toUShort())
        }
    }

    /** The node the release talks to: the poller's last good one, or a fresh
     *  pick — the same order the poller itself uses. */
    private fun node(context: Context): String? =
        NodeStore(context).lastGood() ?: runCatching {
            uniffi.ducat_mobile.moneroPickNode(
                uniffi.ducat_mobile.moneroDefaultNodes(null), "stagenet", 8000u,
            ).url
        }.getOrNull()

    /**
     * Release the escrow back to this device's own wallet: scan, build one
     * sweep, preprocess, and send `[tx][preprocess]` as FrostRound 0 to the
     * other PRINCIPAL — never the arbiter, who only signs when the normal
     * path is dead and someone asks it to.
     */
    fun releaseBond(context: Context, contact: Contact): String {
        val mineHex = PersonaStore(context).personaHex()
        val idHex = all(context)
            .filter { it.optString("peer") == contact.personaHex }
            .lastOrNull { it.optString("stage") == "done" }
            ?.optString("id")
            ?: throw IllegalStateException("no finished bond with this contact")
        val o = load(context, idHex)!!
        val id = hexToBytes(idHex)!!
        val i = o.optInt("i")
        val keys = hexToBytes(o.optString("keys"))
            ?: throw IllegalStateException("this device holds no key share")
        val dest = WalletStore(context).address()
            ?: throw IllegalStateException("no wallet to return the deposit to")
        val nodeUrl = node(context) ?: throw IllegalStateException("no node reachable")

        val roster = o.getJSONArray("roster").let { arr ->
            (0 until arr.length()).map { arr.getString(it) }
        }
        val cosignerIdx = indexOf(roster, contact.personaHex)

        val prop = uniffi.ducat_mobile.frostPropose(
            id, i.toUShort(), keys, dest, nodeUrl,
            WalletStore(context).restoreHeight().toLong().toULong(),
        )
        Mailbox.send(
            context, contact, "bond: returning the deposit",
            mineHex, kind = 9, round = 0, ceremonyId = id, payload = prop.payload,
        )
        o.put("stage", "releasing")
        o.put("cosignerIdx", cosignerIdx)
        o.put("payoutPxmr", prop.payoutPxmr.toLong())
        save(context, idHex, o)
        DucatLog.i(TAG, "bond $idHex: proposed release of ${prop.payoutPxmr} pXMR")
        return idHex
    }

    /**
     * A FrostRound arrived. Round 0 asks this device to co-sign a release;
     * round 1 carries the co-signature back to the proposer, who completes
     * and broadcasts. Whose signature is whose comes from the roster, not
     * from arithmetic — with an arbiter there are three possible pairs.
     */
    @Synchronized
    fun onFrostRound(
        context: Context,
        contact: Contact,
        ceremonyId: ByteArray?,
        round: Long?,
        payload: ByteArray?,
    ) {
        val id = ceremonyId ?: return
        val idHex = id.toHexString()
        payload ?: return
        round ?: return
        val mineHex = PersonaStore(context).personaHex()
        var c = ContactStore(context).all()
            .firstOrNull { it.personaHex == contact.personaHex } ?: contact
        val o = load(context, idHex) ?: return
        val i = o.optInt("i")
        val keys = hexToBytes(o.optString("keys")) ?: return
        val roster = o.optJSONArray("roster")?.let { arr ->
            (0 until arr.length()).map { arr.getString(it) }
        } ?: return
        val senderIdx = indexOf(roster, contact.personaHex)
        if (senderIdx == 0) return
        val stage = o.optString("stage")

        runCatching {
            when {
                (stage == "done" ||
                    (o.optInt("kind") == KIND_RIDE &&
                        stage in listOf("release_pending", "release_cosigned"))) &&
                    round.toInt() == 0 -> {
                    // A ride's release is money moving to the driver, and the
                    // rider's yes is a screen, not an automatic signature —
                    // §15.5's confirm rule surviving into escrow. Park the
                    // proposal; approveRideRelease() is the tap. A fresh
                    // proposal supersedes a parked or even a co-signed one —
                    // the driver re-proposes when a broadcast dies, and the
                    // rider's yes is asked again. A plain bond keeps the
                    // proven auto-cosign (deposit back to its owner).
                    if (o.optInt("kind") == KIND_RIDE) {
                        o.put("stage", "release_pending")
                        o.put("pendingPayload", payload.toHexString())
                        o.put("proposerIdx", senderIdx)
                        save(context, idHex, o)
                        ContactStore.bump()
                        DucatLog.i(TAG, "ride $idHex: release proposed — waiting for the yes")
                        return@runCatching
                    }
                    val ans = uniffi.ducat_mobile.frostCosign(
                        id, i.toUShort(), senderIdx.toUShort(), keys, payload,
                    )
                    c = Mailbox.send(
                        context, c, "bond: co-signed the release",
                        mineHex, kind = 9, round = 1, ceremonyId = id, payload = ans.payload,
                    )
                    o.put("stage", "release_cosigned"); save(context, idHex, o)
                    DucatLog.i(TAG, "bond $idHex: co-signed the release (fee ${ans.feePxmr})")
                }
                stage == "releasing" && round.toInt() == 1 -> {
                    // Only the co-signer we actually asked: in a 2-of-3 an
                    // unsolicited "co-signature" from the third party must
                    // not complete a transaction nobody proposed to them.
                    if (senderIdx != o.optInt("cosignerIdx")) {
                        DucatLog.w(TAG, "bond $idHex: round 1 from unexpected participant $senderIdx")
                        return
                    }
                    val nodeUrl = node(context)
                        ?: throw IllegalStateException("no node reachable")
                    val txid = uniffi.ducat_mobile.frostComplete(
                        id, i.toUShort(), senderIdx.toUShort(), payload, nodeUrl,
                    )
                    o.put("stage", "released"); o.put("txid", txid)
                    save(context, idHex, o)
                    ContactStore.bump()
                    DucatLog.i(TAG, "bond $idHex released — txid $txid")
                }
                else ->
                    DucatLog.w(TAG, "bond $idHex: frost round $round ignored at stage $stage")
            }
        }.onFailure {
            DucatLog.w(TAG, "bond $idHex frost round $round failed: ${it.message}")
        }
    }

    // ===== The ride escrow's own API (§15.12) =====
    //
    // Roles come from the frame, not from who spoke first: the funder index
    // names the rider, the arbiter index names the arbiter, and the one
    // remaining principal is the driver — the payee, the only party a
    // release proposal may pay.

    /** The other principal — never the arbiter. */
    fun otherPrincipal(o: JSONObject): String? {
        val roster = o.optJSONArray("roster")?.let { arr ->
            (0 until arr.length()).map { arr.getString(it) }
        } ?: return null
        val i = o.optInt("i")
        val arb = o.optInt("arbiterIdx")
        return roster.filterIndexed { idx, _ -> idx + 1 != i && idx + 1 != arb }.singleOrNull()
    }

    /** True when this device is the funder — the rider. */
    fun isFunder(o: JSONObject): Boolean = o.optInt("i") == o.optInt("funderIdx")

    /** True when this device is the arbiter — a share and nothing else. */
    fun isArbiter(o: JSONObject): Boolean =
        o.optInt("arbiterIdx") != 0 && o.optInt("i") == o.optInt("arbiterIdx")

    /** The live ride escrow with this contact, if any — newest first, and a
     *  released or aborted one is history, not a banner. */
    fun rideWith(context: Context, peerHex: String): JSONObject? =
        all(context)
            .filter { it.optInt("kind") == KIND_RIDE && !isArbiter(it) }
            .filter { otherPrincipal(it) == peerHex }
            .sortedByDescending { it.optLong("created") }
            .firstOrNull { it.optString("stage") !in listOf("released", "aborted") }

    /** The rider pays the fare into the escrow — an ordinary wallet send to
     *  an address that happens to need two of three keys to leave. */
    fun fundRide(context: Context, idHex: String): String {
        val o = load(context, idHex) ?: throw IllegalStateException("no such ceremony")
        check(o.optString("stage") == "done") { "the escrow is not built yet" }
        check(isFunder(o)) { "only the rider funds the fare" }
        val addr = o.optString("address")
        val fare = o.optLong("farePxmr")
        check(addr.isNotEmpty() && fare > 0) { "no address or fare" }
        val nodeUrl = node(context) ?: throw IllegalStateException("no node reachable")
        val r = Wallet.send(context, nodeUrl, addr, fare)
        o.put("fundTxid", r.txidHex)
        save(context, idHex, o)
        ContactStore.bump()
        DucatLog.i(TAG, "ride $idHex: fare ${formatXmr(fare)} XMR sent to escrow — ${r.txidHex.take(16)}…")
        return r.txidHex
    }

    /**
     * Ask the chain what the escrow holds — this device's own scan (§17.5),
     * which is how "fare secured" is a fact rather than a claim. Records the
     * figure; returns it.
     */
    fun checkRideFunding(context: Context, idHex: String): Long {
        val o = load(context, idHex) ?: return 0
        val keys = hexToBytes(o.optString("keys")) ?: return 0
        val nodeUrl = node(context) ?: return o.optLong("fundedPxmr")
        val from = o.optLong("scanFrom").takeIf { it > 0 }
            ?: WalletStore(context).restoreHeight().toLong()
        val bal = runCatching {
            uniffi.ducat_mobile.escrowBalance(keys, nodeUrl, from.toULong()).toLong()
        }.getOrElse { return o.optLong("fundedPxmr") }
        if (bal != o.optLong("fundedPxmr")) {
            o.put("fundedPxmr", bal)
            save(context, idHex, o)
            ContactStore.bump()
            if (bal > 0) DucatLog.i(TAG, "ride $idHex: escrow holds ${formatXmr(bal)} XMR")
        }
        return bal
    }

    /**
     * The driver marks the ride complete: propose the release, destination
     * this device's own wallet — the payee proposing to be paid, which is
     * why the missing co-signer consent view does not bite here: the sweep
     * can only go where the proposer says, and the proposer is the party
     * the fare was always for.
     */
    fun proposeRideRelease(context: Context, idHex: String): Long {
        val o = load(context, idHex) ?: throw IllegalStateException("no such ceremony")
        // "releasing" is retryable: a broadcast can die on the node ("no relay
        // took the release", found live) and a fresh proposal — new nonces,
        // same inputs — is always safe. What is not retryable is done money:
        // released is final.
        check(o.optString("stage") in listOf("done", "releasing")) {
            "the escrow is not ready to release"
        }
        check(!isFunder(o) && !isArbiter(o)) { "only the driver proposes the fare's release" }
        val id = hexToBytes(idHex)!!
        val i = o.optInt("i")
        val keys = hexToBytes(o.optString("keys"))
            ?: throw IllegalStateException("this device holds no key share")
        val dest = WalletStore(context).address()
            ?: throw IllegalStateException("no wallet to receive the fare")
        val nodeUrl = node(context) ?: throw IllegalStateException("no node reachable")
        val from = o.optLong("scanFrom").takeIf { it > 0 }
            ?: WalletStore(context).restoreHeight().toLong()
        val riderHex = otherPrincipal(o) ?: throw IllegalStateException("no counterparty")
        val rider = contactFor(context, riderHex)
            ?: throw IllegalStateException("the rider is not a contact")

        val prop = uniffi.ducat_mobile.frostPropose(
            id, i.toUShort(), keys, dest, nodeUrl, from.toULong(),
        )
        Mailbox.send(
            context, rider, "ride complete — requesting the fare",
            PersonaStore(context).personaHex(),
            kind = 9, round = 0, ceremonyId = id, payload = prop.payload,
        )
        o.put("stage", "releasing")
        o.put("cosignerIdx", indexOf(
            o.getJSONArray("roster").let { arr -> (0 until arr.length()).map { arr.getString(it) } },
            riderHex,
        ))
        o.put("payoutPxmr", prop.payoutPxmr.toLong())
        save(context, idHex, o)
        ContactStore.bump()
        DucatLog.i(TAG, "ride $idHex: proposed release of ${prop.payoutPxmr} pXMR to the driver")
        return prop.payoutPxmr.toLong()
    }

    /**
     * The rider's yes: co-sign the parked proposal and send the signature
     * back. This is the §15 moment of the bonded ride — both parties present,
     * the tap that moves the money.
     */
    fun approveRideRelease(context: Context, idHex: String) {
        val o = load(context, idHex) ?: throw IllegalStateException("no such ceremony")
        check(o.optString("stage") == "release_pending") { "no release is waiting" }
        val id = hexToBytes(idHex)!!
        val i = o.optInt("i")
        val keys = hexToBytes(o.optString("keys"))
            ?: throw IllegalStateException("this device holds no key share")
        val payload = hexToBytes(o.optString("pendingPayload"))
            ?: throw IllegalStateException("the proposal is gone")
        val proposerIdx = o.optInt("proposerIdx")
        val roster = o.getJSONArray("roster").let { arr ->
            (0 until arr.length()).map { arr.getString(it) }
        }
        val proposerHex = roster.getOrNull(proposerIdx - 1)
            ?: throw IllegalStateException("no proposer")
        val proposer = contactFor(context, proposerHex)
            ?: throw IllegalStateException("the driver is not a contact")

        val ans = uniffi.ducat_mobile.frostCosign(
            id, i.toUShort(), proposerIdx.toUShort(), keys, payload,
        )
        Mailbox.send(
            context, proposer, "fare released — thank you for the ride",
            PersonaStore(context).personaHex(),
            kind = 9, round = 1, ceremonyId = id, payload = ans.payload,
        )
        o.put("stage", "release_cosigned")
        o.remove("pendingPayload")
        save(context, idHex, o)
        ContactStore.bump()
        DucatLog.i(TAG, "ride $idHex: rider approved the release (fee ${ans.feePxmr})")
    }
}
