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
    // [u8 nonceLen][nonce][commitment…]
    //
    // The same format for two parties and three: one parser, no special
    // cases, and a 2-party bond is simply a roster with no arbiter.

    private fun frameRound0(
        roster: List<String>,
        arbiterIdx: Int,
        nonce: String,
        commitment: ByteArray,
    ): ByteArray {
        val out = java.io.ByteArrayOutputStream()
        out.write(roster.size)
        roster.forEach { out.write(hexToBytes(it)!!) }
        out.write(arbiterIdx)
        val nb = nonce.toByteArray()
        out.write(nb.size)
        out.write(nb)
        out.write(commitment)
        return out.toByteArray()
    }

    private data class Invite(
        val roster: List<String>,
        val arbiterIdx: Int,
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
        val nonceLen = take(1)?.get(0)?.toInt() ?: return null
        val nonce = String(take(nonceLen) ?: return null)
        val commitment = payload.copyOfRange(p, payload.size)
        if (commitment.isEmpty()) return null
        return Invite(roster, arbiterIdx, nonce, commitment)
    }

    private fun contactFor(context: Context, personaHex: String): Contact? =
        ContactStore(context).all().firstOrNull { it.personaHex == personaHex }

    /**
     * Start a bond with a contact — and optionally an arbiter, who must be a
     * mutual contact of both sides (the shares travel the threads; missing
     * threads mean an impossible ceremony, discovered by whoever lacks one).
     */
    fun startBond(context: Context, contact: Contact, arbiter: Contact? = null): String {
        val mineHex = PersonaStore(context).personaHex()
        val roster = buildList {
            add(mineHex); add(contact.personaHex); arbiter?.let { add(it.personaHex) }
        }.sorted()
        val arbiterIdx = arbiter?.let { indexOf(roster, it.personaHex) } ?: 0
        val nonce = java.util.UUID.randomUUID().toString().take(8)
        val id = ceremonyId(roster, nonce)
        val idHex = id.toHexString()
        val i = indexOf(roster, mineHex)
        val n = roster.size

        val commit = uniffi.ducat_mobile.dkgCommit(id, i.toUShort(), T.toUShort(), n.toUShort())
        val frame = frameRound0(roster, arbiterIdx, nonce, commit)
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
            put("peer", contact.personaHex)
            put("i", i); put("stage", "committed")
            put("commits", JSONObject()); put("shares", JSONObject())
        }
        save(context, idHex, o)
        DucatLog.i(TAG, "started bond $idHex (i=$i of $n), sent commitment")
        return idHex
    }

    /**
     * A DkgRound arrived. Record it, and advance the engine when everything
     * it was waiting for is in. Out-of-stage rounds are ignored (§2.5).
     */
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
            val frame = frameRound0(inv.roster, inv.arbiterIdx, inv.nonce, commit)
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
                save(context, idHex, o)
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
                stage == "done" && round.toInt() == 0 -> {
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
                    DucatLog.i(TAG, "bond $idHex released — txid $txid")
                }
                else ->
                    DucatLog.w(TAG, "bond $idHex: frost round $round ignored at stage $stage")
            }
        }.onFailure {
            DucatLog.w(TAG, "bond $idHex frost round $round failed: ${it.message}")
        }
    }
}
