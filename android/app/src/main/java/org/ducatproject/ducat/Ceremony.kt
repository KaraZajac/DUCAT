package org.ducatproject.ducat

import android.content.Context
import org.json.JSONObject

/**
 * The bond ceremony's orchestration (§17.9): the glue between the sealed
 * thread and the threshold engine in the bridge.
 *
 * The crypto lives in Rust (`ceremony.rs`, one machine per round held by
 * ceremony id); this drives it. Starting a bond runs `dkgCommit` and seals
 * the commitment as a `DkgRound` message. Every `DkgRound` that arrives on a
 * thread is handed here, which advances the engine when the round it was
 * waiting for lands: commit → share → finish, each step a sealed message the
 * counterparty answers in kind. The finished escrow — its funding address and
 * this device's key share — is stored, and no other party's share is ever
 * on this device.
 *
 * State survives process death in prefs, because a ceremony spans poll cycles
 * and app restarts; the in-memory engine machines do not survive, so a
 * restart mid-ceremony aborts cleanly (the peer times out and both retry) —
 * "nothing happens" is not left as a silent default (§9.3.4).
 */
object Ceremony {
    private const val TAG = "DucatCeremony"

    // 2-of-2 for a bond: the rider and one counterparty (an arbiter, or the
    // other side of a two-party escrow). 2-of-3 with an arbiter set is the
    // same flow with n=3 and a second peer.
    private const val T = 2
    private const val N = 2

    private fun prefs(context: Context) =
        context.getSharedPreferences("ducat_ceremonies", Context.MODE_PRIVATE)

    /** A ceremony in progress or finished, one JSON object per ceremony id. */
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
     * The 32-byte context both sides must agree on, derived from the two
     * personas and a nonce so a fresh bond never collides with an old one.
     * Lower persona hex is participant 1 — a rule both compute identically
     * without negotiating.
     */
    private fun ceremonyId(mineHex: String, theirsHex: String, nonce: String): ByteArray {
        val lo = if (mineHex < theirsHex) mineHex else theirsHex
        val hi = if (mineHex < theirsHex) theirsHex else mineHex
        val md = java.security.MessageDigest.getInstance("SHA-256")
        md.update("DUCAT-BOND-v0".toByteArray())
        md.update(lo.toByteArray()); md.update(hi.toByteArray()); md.update(nonce.toByteArray())
        return md.digest()
    }

    private fun myParticipant(mineHex: String, theirsHex: String): Int =
        if (mineHex < theirsHex) 1 else 2

    private fun theirParticipant(mineHex: String, theirsHex: String): Int =
        if (mineHex < theirsHex) 2 else 1

    /**
     * Start a bond with a contact: commit, seal the commitment as the first
     * DkgRound, and record the ceremony as awaiting the peer's commitment.
     */
    fun startBond(context: Context, contact: Contact): String {
        val mineHex = PersonaStore(context).personaHex()
        val theirsHex = contact.personaHex
        val nonce = java.util.UUID.randomUUID().toString().take(8)
        val id = ceremonyId(mineHex, theirsHex, nonce)
        val idHex = id.toHexString()
        val i = myParticipant(mineHex, theirsHex)

        val commit = uniffi.ducat_mobile.dkgCommit(id, i.toUShort(), T.toUShort(), N.toUShort())
        Mailbox.send(
            context, contact, "bond: building a shared deposit",
            mineHex, kind = 8, round = 0, ceremonyId = id, payload = commit,
        )
        val o = JSONObject().apply {
            put("id", idHex); put("nonce", nonce); put("peer", theirsHex)
            put("i", i); put("stage", "committed")
        }
        save(context, idHex, o)
        DucatLog.i(TAG, "started bond $idHex (i=$i), sent commitment")
        return idHex
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
     * Release the escrow back to this device's own wallet (§17.9's FROST
     * side): scan, build one sweep, preprocess, and send `[tx][preprocess]`
     * as FrostRound 0. The peer co-signs sight-unseen for now — the consent
     * view waits on a payments accessor upstream — and the completion lands
     * back here as round 1.
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

        val prop = uniffi.ducat_mobile.frostPropose(
            id, i.toUShort(), keys, dest, nodeUrl,
            WalletStore(context).restoreHeight().toLong().toULong(),
        )
        Mailbox.send(
            context, contact, "bond: returning the deposit",
            mineHex, kind = 9, round = 0, ceremonyId = id, payload = prop.payload,
        )
        o.put("stage", "releasing")
        o.put("payoutPxmr", prop.payoutPxmr.toLong())
        save(context, idHex, o)
        DucatLog.i(TAG, "bond $idHex: proposed release of ${prop.payoutPxmr} pXMR")
        return idHex
    }

    /**
     * A FrostRound arrived. Round 0 asks this device to co-sign the release;
     * round 1 carries the co-signature back to the proposer, who completes
     * and broadcasts. Out-of-stage rounds are ignored, like the DKG's.
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
        val stage = o.optString("stage")

        runCatching {
            when {
                stage == "done" && round.toInt() == 0 -> {
                    val ans = uniffi.ducat_mobile.frostCosign(id, i.toUShort(), keys, payload)
                    c = Mailbox.send(
                        context, c, "bond: co-signed the release",
                        mineHex, kind = 9, round = 1, ceremonyId = id, payload = ans.payload,
                    )
                    o.put("stage", "release_cosigned"); save(context, idHex, o)
                    DucatLog.i(TAG, "bond $idHex: co-signed the release (fee ${ans.feePxmr})")
                }
                stage == "releasing" && round.toInt() == 1 -> {
                    val nodeUrl = node(context)
                        ?: throw IllegalStateException("no node reachable")
                    val txid = uniffi.ducat_mobile.frostComplete(
                        id, i.toUShort(), payload, nodeUrl,
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

    /**
     * A DkgRound arrived. Advance the engine if it is the round this ceremony
     * was waiting for. Unknown or out-of-stage rounds are ignored (§2.5: a
     * ceremony message is never applied out of order).
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
        val theirsHex = contact.personaHex
        val i = myParticipant(mineHex, theirsHex)
        val theirI = theirParticipant(mineHex, theirsHex)

        // Every send returns the contact with its outbox sequence advanced,
        // and the NEXT send must use that one. Reusing the argument sent two
        // rounds as the same seq in one poll cycle — the share overwrote the
        // commitment in the ring, and the peer only ever saw the second
        // (found live, first two-phone run, 2026-08-16). The argument can be
        // stale the same way when two rounds arrive in one poll, so start
        // from the store's copy, not the caller's.
        var c = ContactStore(context).all()
            .firstOrNull { it.personaHex == contact.personaHex } ?: contact

        // A peer's round-0 with no ceremony of ours is an invitation: commit
        // in response so both sides converge, then treat their round-0 as the
        // one we were waiting for.
        var o = load(context, idHex)
        if (o == null && round.toInt() == 0) {
            val commit = uniffi.ducat_mobile.dkgCommit(id, i.toUShort(), T.toUShort(), N.toUShort())
            c = Mailbox.send(
                context, c, "bond: building a shared deposit",
                mineHex, kind = 8, round = 0, ceremonyId = id, payload = commit,
            )
            o = JSONObject().apply {
                put("id", idHex); put("peer", theirsHex); put("i", i); put("stage", "committed")
            }
            save(context, idHex, o)
            DucatLog.i(TAG, "joined bond $idHex (i=$i), sent commitment")
        }
        o = o ?: return

        val stage = o.optString("stage")
        val from = listOf(uniffi.ducat_mobile.FromParty(theirI.toUShort(), payload))

        runCatching {
            when {
                stage == "committed" && round.toInt() == 0 -> {
                    val shares = uniffi.ducat_mobile.dkgShare(
                        id, i.toUShort(), T.toUShort(), N.toUShort(), from,
                    )
                    val mine = shares.firstOrNull { it.participant.toInt() == theirI } ?: return
                    c = Mailbox.send(
                        context, c, "bond: your share",
                        mineHex, kind = 8, round = 1, ceremonyId = id, payload = mine.bytes,
                    )
                    o.put("stage", "shared"); save(context, idHex, o)
                    DucatLog.i(TAG, "bond $idHex: shared, sent our share")
                }
                stage == "shared" && round.toInt() == 1 -> {
                    val addr = uniffi.ducat_mobile.dkgFinish(
                        id, i.toUShort(), T.toUShort(), N.toUShort(), from, true,
                    )
                    val keys = uniffi.ducat_mobile.dkgTakeKeys(id, i.toUShort())
                    o.put("stage", "done"); o.put("address", addr)
                    o.put("keys", keys.toHexString())
                    save(context, idHex, o)
                    DucatLog.i(TAG, "bond $idHex done — escrow $addr")
                }
                else ->
                    DucatLog.w(TAG, "bond $idHex: round $round ignored at stage $stage")
            }
        }.onFailure {
            DucatLog.w(TAG, "bond $idHex round $round failed: ${it.message}")
            uniffi.ducat_mobile.ceremonyAbort(id, i.toUShort())
        }
    }
}
