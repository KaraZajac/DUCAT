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

    /**
     * What each in-flight record looked like when it was read.
     *
     * Weak and keyed by identity — JSONObject does not override equals — so a
     * ceremony nobody is holding falls out on its own. See [save] for what
     * this is for.
     */
    private val loadedAs = java.util.WeakHashMap<JSONObject, String>()

    private fun load(context: Context, id: String): JSONObject? =
        prefs(context).getString("c_$id", null)?.let { raw ->
            JSONObject(raw).also { synchronized(lock) { loadedAs[it] = raw } }
        }

    /**
     * Write our changes without discarding somebody else's.
     *
     * Six functions here read a ceremony, do something slow — build a
     * transaction, run a FROST round, send a message — and write the record
     * back afterwards. Meanwhile the poller is landing protocol rounds into
     * the same record on its own thread. A plain write puts back a snapshot
     * taken before all of that, so whichever finished last silently undid the
     * other: a DKG round overwritten is a ceremony that stalls with nothing
     * to restart it, and a funding mark overwritten is a second tap paying a
     * second time into an escrow that needs a co-signature to give anything
     * back.
     *
     * So this compares the record against the version it was read from, and
     * lays only the fields that *this* caller actually changed over whatever
     * is on disk now. Restructuring six protocol functions to re-read after
     * their slow part would be the other way to fix it, one function at a
     * time and one mistake at a time; this fixes the shape.
     *
     * Fields are added and changed here, never removed — which is all this
     * code does to a ceremony, and worth knowing before the first caller
     * tries to.
     */
    private fun save(context: Context, id: String, o: JSONObject) = synchronized(lock) {
        val merged = mergeOnto(loadedAs[o], prefs(context).getString("c_$id", null), o)
        if (merged !== o) DucatLog.i(TAG, "ceremony $id: merged onto a record that moved")
        val text = merged.toString()
        prefs(context).edit().putString("c_$id", text).apply()
        loadedAs[o] = text
    }

    /**
     * Guards load-change-save on one ceremony.
     *
     * Two threads write these. Protocol rounds arrive on the poller and land
     * through [onDkgRound] and [onFrostRound]; funding, signing and releasing
     * run from a screen. Both read the whole record, change a field and write
     * the whole record back, so without this the loser's write is silently
     * undone — and here that is not a cosmetic loss. A DKG round overwritten
     * is a ceremony that stalls with no way to restart it; a `fundTxid`
     * overwritten is the only thing standing between a second tap and a
     * second payment into an escrow that needs a co-signature to give
     * anything back.
     */
    private val lock = Any()

    /**
     * Held in a party's txid slot while their transaction is being built.
     *
     * Not a transaction id and never mistaken for one: everything that reads
     * these fields asks whether they are empty, and this is not empty. It
     * marks the slot taken so a second tap cannot start a second payment
     * during the seconds the first one spends talking to a node.
     */
    private const val SENDING = "sending"

    /**
     * Change a ceremony under the lock, on a record read *inside* it.
     *
     * The reading matters as much as the locking. Every caller that mutated
     * one of these held a JSONObject loaded before it did its slow work — a
     * transaction build, a FROST round, a message send — and wrote that
     * snapshot back afterwards, discarding whatever had arrived in between.
     * Do the slow part first, then come in here and change what is actually
     * on disk.
     */
    private fun <T> mutate(context: Context, id: String, f: (JSONObject) -> T): T =
        synchronized(lock) {
            val o = load(context, id) ?: throw IllegalStateException("no such ceremony")
            val r = f(o)
            save(context, id, o)
            r
        }

    /**
     * The rule [save] applies: our changes, laid over theirs.
     *
     * [readFrom] is the text this record was read from, [onDisk] what is there
     * now, [now] the record as this caller has changed it. When the two agree
     * there was no other writer and [now] is returned unchanged. When they do
     * not, only the fields this caller actually touched — the ones that differ
     * from what it read — go onto the newer record.
     *
     * Separate and pure so it can be checked directly; the interesting cases
     * are all about what two writers each did, and none of them need a disk.
     */
    internal fun mergeOnto(readFrom: String?, onDisk: String?, now: JSONObject): JSONObject {
        if (readFrom == null || onDisk == null || onDisk == readFrom) return now
        val theirs = JSONObject(onDisk)
        val was = JSONObject(readFrom)
        now.keys().forEach { k ->
            if (was.opt(k)?.toString() != now.opt(k)?.toString()) theirs.put(k, now.get(k))
        }
        return theirs
    }

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
    // [u64 farePxmr LE][u8 refundLen][refund addr]
    // [u64 funderDepPxmr LE][u64 hostDepPxmr LE]
    // [u8 nonceLen][nonce][commitment…]
    //
    // The refund address is where the funder's residual comes home in the
    // split release — the margin above the fare, a deposit, a negotiated
    // slice. A fresh subaddress per ceremony, so nothing links two rides.
    // Empty for a plain bond (a sweep needs no second destination).
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
    /** A reservation (§15.12's Airbnb/Turo shape): the guest funds rent plus
     *  their deposit, the host funds a deposit of their own — funding IS the
     *  host's acceptance — and the default release sends each deposit home
     *  beside the rent. Same consent gates, settlement and ruling as a ride. */
    const val KIND_RESERVATION = 2

    private fun frameRound0(
        roster: List<String>,
        arbiterIdx: Int,
        kind: Int,
        funderIdx: Int,
        farePxmr: Long,
        refundAddr: String,
        funderDepPxmr: Long,
        hostDepPxmr: Long,
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
        val rb = refundAddr.toByteArray()
        out.write(rb.size)
        out.write(rb)
        for (v in listOf(funderDepPxmr, hostDepPxmr)) {
            out.write(
                java.nio.ByteBuffer.allocate(8)
                    .order(java.nio.ByteOrder.LITTLE_ENDIAN).putLong(v).array()
            )
        }
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
        val refundAddr: String,
        val funderDepPxmr: Long,
        val hostDepPxmr: Long,
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
        val refundLen = take(1)?.get(0)?.toInt() ?: return null
        val refund = String(take(refundLen) ?: return null)
        fun u64(): Long? = take(8)?.let {
            java.nio.ByteBuffer.wrap(it).order(java.nio.ByteOrder.LITTLE_ENDIAN).long
        }
        val fDep = u64() ?: return null
        val hDep = u64() ?: return null
        val nonceLen = take(1)?.get(0)?.toInt() ?: return null
        val nonce = String(take(nonceLen) ?: return null)
        val commitment = payload.copyOfRange(p, payload.size)
        if (commitment.isEmpty()) return null
        return Invite(roster, arbiterIdx, kind, funderIdx, fare, refund, fDep, hDep, nonce, commitment)
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
     * echoed.
     *
     * The escrow ladder's top two rungs, one entry point: with an arbiter, a
     * 2-of-3 — a lost phone strands nothing and a dispute has somewhere to
     * go. Without one, a 2-of-2 on mutual stakes: the rider's margin above
     * the fare is what makes releasing strictly better than sulking, and
     * walking away burns both sides. Nobody is blocked because a third party
     * does not exist; the third party is simply better when it does.
     */
    fun startRide(
        context: Context,
        driver: Contact,
        arbiter: Contact?,
        farePxmr: Long,
        /**
         * What the driver puts in beside the fare, if anything.
         *
         * Zero is the one-sided ride: the rider funds, and the driver's skin
         * is the fare they forfeit by not finishing. A non-zero stake makes
         * it symmetric — both sides have money in the pot, and the default
         * release hands each their own back with the fare going to the
         * driver. Nothing downstream needs to know which it was: the stake
         * simply joins the escrow and comes home in the residual.
         */
        driverStakePxmr: Long = 0L,
    ): String =
        start(
            context, driver, arbiter, KIND_RIDE, farePxmr,
            // Named in the frame, not recomputed later: an escrow states its
            // own arithmetic, and a suggested percentage that changes next
            // month must not re-price a ride that is already standing.
            funderDepPxmr = Stakes.stakeFor(Stakes.Deal.Ride, farePxmr),
            hostDepPxmr = driverStakePxmr,
        )

    /**
     * What the rider locks: the fare plus their own stake.
     *
     * The percentage lives in [Stakes], with the reasoning and the sources
     * beside it, because this number is the whole trust argument and must
     * not be three constants in three screens. A ride's stake is symmetric —
     * the driver posts the same — so the sentence a user has to understand
     * is one sentence: you both put up a stake, and finishing gives it back.
     */
    /**
     * What the core holds back to pay for the release transaction.
     *
     * Mirrors `FEE_RESERVE` in mobile/src/ceremony.rs. Whichever side is paid
     * the *residual* of a split pays the fee out of its own share, so an
     * escrow whose residual side is smaller than this cannot be released at
     * all — and by then the money is already in it.
     */
    const val FEE_RESERVE_PXMR: Long = 200_000_000L

    /**
     * The smallest price worth putting in an escrow.
     *
     * Twice the reserve, so the side taking the residual is left with
     * something after the fee rather than exactly nothing. Below this the
     * release costs more than the thing being sold, which is not a deal the
     * app should let anybody fund.
     */
    const val MIN_ESCROW_PXMR: Long = FEE_RESERVE_PXMR * 2

    fun rideFundAmount(farePxmr: Long): Long =
        Stakes.funderLocks(Stakes.Deal.Ride, farePxmr)

    /** The stake the driver is asked for on a ride: the rider's, mirrored. */
    fun rideStakeAmount(farePxmr: Long): Long =
        Stakes.providerLocks(Stakes.Deal.Ride, farePxmr)

    /**
     * Start a reservation escrow (§15.12's Airbnb/Turo shape): the caller is
     * the guest — the funder — the contact is the host. Rent and both
     * deposits ride the frame, so the escrow names its whole arithmetic and
     * the host's phone can state exactly what accepting costs. The host's
     * acceptance IS funding their deposit: until money moves, nothing is at
     * risk and nothing needs a signature.
     */
    fun startReservation(
        context: Context,
        host: Contact,
        arbiter: Contact?,
        rentPxmr: Long,
        guestDepPxmr: Long,
        hostDepPxmr: Long,
    ): String = start(context, host, arbiter, KIND_RESERVATION, rentPxmr, guestDepPxmr, hostDepPxmr)

    /** What a fully funded escrow holds: ride = fare + margin; reservation =
     *  rent + both deposits. */
    fun expectedTotalPxmr(o: JSONObject): Long = when (o.optInt("kind")) {
        KIND_RESERVATION ->
            o.optLong("farePxmr") + o.optLong("funderDepPxmr") + o.optLong("hostDepPxmr")
        // A ride's pot is what the rider locks plus whatever the driver
        // staked beside it — zero on the one-sided ride, which is the same
        // arithmetic it has always been.
        else -> o.optLong("farePxmr") +
            o.optLong("funderDepPxmr").takeIf { it > 0 }
                .let { it ?: (rideFundAmount(o.optLong("farePxmr")) - o.optLong("farePxmr")) } +
            o.optLong("hostDepPxmr")
    }

    /** What THIS party still owes the escrow: the funder their side, the
     *  host (reservations only) their deposit. */
    fun mySharePxmr(o: JSONObject): Long = when {
        o.optInt("kind") == KIND_RESERVATION && isFunder(o) ->
            o.optLong("farePxmr") + o.optLong("funderDepPxmr")
        o.optInt("kind") == KIND_RESERVATION && !isArbiter(o) -> o.optLong("hostDepPxmr")
        isFunder(o) -> o.optLong("farePxmr") +
            o.optLong("funderDepPxmr").takeIf { it > 0 }
                .let { it ?: (rideFundAmount(o.optLong("farePxmr")) - o.optLong("farePxmr")) }
        // The driver on a two-sided ride owes exactly their stake.
        !isArbiter(o) -> o.optLong("hostDepPxmr")
        else -> 0L
    }

    /**
     * The part of my share that comes home — my stake, not the price.
     *
     * Read from the record rather than recomputed, because the two sides
     * *agreed* these numbers: the booking sheet suggests them from the deal
     * table and then lets either side type over the suggestion. Anything that
     * works them out again from the fare is answering a different question.
     *
     * The banner did exactly that, and with one deal for every reservation —
     * a room's twenty percent, whatever was actually being agreed. So the note
     * beside the button asking somebody to commit money quoted double the
     * truth for a bicycle (ten percent), two thirds of it for a car (thirty),
     * and any figure at all for a stake the two of them had negotiated.
     */
    fun myStakePxmr(o: JSONObject): Long = when {
        isArbiter(o) -> 0L
        isFunder(o) -> o.optLong("funderDepPxmr").takeIf { it > 0 }
            ?: (rideFundAmount(o.optLong("farePxmr")) - o.optLong("farePxmr"))
                .coerceAtLeast(0L)
        else -> o.optLong("hostDepPxmr")
    }

    private fun start(
        context: Context,
        contact: Contact,
        arbiter: Contact?,
        kind: Int,
        farePxmr: Long,
        funderDepPxmr: Long = 0L,
        hostDepPxmr: Long = 0L,
    ): String {
        val mineHex = PersonaStore(context).personaHex()
        val roster = buildList {
            add(mineHex); add(contact.personaHex); arbiter?.let { add(it.personaHex) }
        }.sorted()
        val arbiterIdx = arbiter?.let { indexOf(roster, it.personaHex) } ?: 0
        val funderIdx = indexOf(roster, mineHex)
        // 128 bits, not the 32 that `UUID…take(8)` was giving. The ceremony
        // id is SHA-256 over the roster and this nonce, and it is the DKG's
        // context string as well as the seed for the funder's refund
        // subaddress — so its requirement is uniqueness, not secrecy, and two
        // ceremonies colliding would share a refund address ("so two rides
        // never link" is the comment below) and collide in the engine's
        // (ceremony_id, i) state map while one is still in flight. For a
        // fixed roster, eight hex characters put the birthday bound around
        // 65k ceremonies; the full sixteen bytes cost nothing.
        val nonce = ByteArray(16)
            .also(java.security.SecureRandom()::nextBytes)
            .joinToString("") { "%02x".format(it) }
        val id = ceremonyId(roster, nonce)
        val idHex = id.toHexString()
        val i = funderIdx
        val n = roster.size
        // Where the funder's residual comes home in the split release: a
        // fresh subaddress per ceremony, so two rides never link.
        val refundAddr = if (kind != KIND_BOND) {
            WalletStore(context).addressFor("ride_$idHex") ?: ""
        } else ""

        val commit = uniffi.ducat_mobile.dkgCommit(id, i.toUShort(), T.toUShort(), n.toUShort())
        val frame = frameRound0(
            roster, arbiterIdx, kind, funderIdx, farePxmr, refundAddr,
            funderDepPxmr, hostDepPxmr, nonce, commit,
        )
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
            put("refundAddr", refundAddr)
            put("funderDepPxmr", funderDepPxmr); put("hostDepPxmr", hostDepPxmr)
            put("created", System.currentTimeMillis())
            put("peer", contact.personaHex)
            // What this deal is about, as it stood when it was struck.
            //
            // The thread's subject moves on — the same neighbour can sell you
            // a grinder in March and fix your bike in April — so a booking
            // that reads it back later would relabel itself with whatever is
            // being discussed now. A settled deal is a record, and a record
            // that changes its own subject is not one.
            Enquiries.about(context, contact.personaHex)?.let { a ->
                put("aboutTitle", a.title)
                put("aboutKind", a.kind)
            }
            put("i", i); put("stage", "committed")
            put("commits", JSONObject()); put("shares", JSONObject())
        }
        save(context, idHex, o)
        DucatLog.i(TAG, "started ${when (kind) {
            KIND_RIDE -> "ride escrow"; KIND_RESERVATION -> "reservation escrow"; else -> "bond"
        }} $idHex (i=$i of $n)")
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
                inv.refundAddr, inv.funderDepPxmr, inv.hostDepPxmr, inv.nonce, commit,
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
                put("refundAddr", inv.refundAddr)
                put("funderDepPxmr", inv.funderDepPxmr); put("hostDepPxmr", inv.hostDepPxmr)
                put("created", System.currentTimeMillis())
                put("peer", contact.personaHex)
                // The same snapshot [start] takes, taken by the side that
                // joins. It was only ever written by the initiator, so a
                // host's copy of every deal carried no subject at all — and a
                // host is the party who *always* joins. It went unnoticed
                // because the fallback reads the thread, and for the newest
                // deal with somebody the thread is still about the right
                // thing. The second deal with the same neighbour is what
                // exposes it: their bike repair relabelled itself "Unnamed
                // contact" the moment a kayak was asked about, on the phone
                // belonging to the person who did the work.
                Enquiries.about(context, contact.personaHex)?.let { a ->
                    put("aboutTitle", a.title)
                    put("aboutKind", a.kind)
                }
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
        val nodeUrl = node(context) ?: throw NoNode()

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
        /** What the proposal claims the funder gets back — display for the
         *  consent screen. The signature is over the payload; this is the
         *  statement beside it (§15.12: the claimed split, where it signs). */
        riderBackPxmr: Long? = null,
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
                    (o.optInt("kind") != KIND_BOND &&
                        stage in listOf("releasing", "release_pending", "release_cosigned"))) &&
                    round.toInt() == 0 -> {
                    // A ride's release is money moving, and the other side's
                    // yes is a screen, not an automatic signature — §15.5's
                    // confirm rule surviving into escrow. Park the proposal;
                    // approveRideRelease() is the tap. A fresh proposal
                    // supersedes a parked, co-signed, or even our own
                    // outstanding one — that last case is the counter-offer:
                    // both sides can propose, and whoever signs ends it. A
                    // plain bond keeps the proven auto-cosign.
                    if (o.optInt("kind") != KIND_BOND) {
                        o.put("stage", "release_pending")
                        o.put("pendingPayload", payload.toHexString())
                        o.put("proposerIdx", senderIdx)
                        if (riderBackPxmr != null) o.put("pendingRiderBack", riderBackPxmr)
                        else o.remove("pendingRiderBack")
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
                    settle(o, "release_cosigned"); save(context, idHex, o)
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
                        ?: throw NoNode()
                    val txid = uniffi.ducat_mobile.frostComplete(
                        id, i.toUShort(), senderIdx.toUShort(), payload, nodeUrl,
                    )
                    settle(o, "released"); o.put("txid", txid)
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

    /**
     * How long a settled escrow keeps the banner: long enough that the person
     * who just finished a ride sees it land, short enough that yesterday's
     * ride is not still on today's screen.
     */
    private const val SETTLED_SHOWN_SECS = 24 * 60 * 60L

    /** The ride escrow with this contact that the banner should be about. */
    /**
     * True when this escrow is over: paid out, signed away, or abandoned.
     *
     * [rideWith] deliberately returns the newest escrow whatever state it is
     * in, because the banner narrates the newest deal. Anything asking "are
     * these two in the middle of something" has to say so itself — asking
     * only whether an escrow exists means yes for ever after the first one.
     */
    fun isFinished(o: JSONObject): Boolean =
        o.optString("stage") in setOf("released", "release_cosigned", "aborted")

    /** How long a proposal nobody answered stays "probably just slow". */
    private const val UNANSWERED_MS = 30L * 60 * 1000

    /**
     * True when this escrow has nothing at stake and nobody ever answered it.
     *
     * "aborted" is read in three places in this app and written in none: there
     * is no decline, no cancel, and no expiry, so a proposal the other side
     * simply never accepts stays unfinished for ever. That would be untidy on
     * its own; what makes it a trap is that the button offering to propose the
     * *next* deal is hidden while a deal is live. Ask a neighbour to rent you
     * a kayak, have them never answer, and neither of you can ever propose
     * anything to the other again — on the screen whose whole purpose is
     * agreeing the next one. The last fix to that sentence caught the settled
     * case and left this one.
     *
     * Nothing here abandons an escrow with money in it. The test is this
     * device's own scan of the escrow address (§17.5) plus the two txids it
     * would have written itself, so an escrow either side has funded is never
     * stale, and the way out of one of those is the release or the arbiter —
     * not a screen quietly deciding it is over.
     */
    fun isStale(o: JSONObject): Boolean {
        if (isFinished(o)) return false
        if (o.optLong("fundedPxmr") > 0) return false
        if (o.optString("fundTxid").isNotEmpty()) return false
        if (o.optString("hostFundTxid").isNotEmpty()) return false
        val created = o.optLong("created").takeIf { it > 0 } ?: return false
        return System.currentTimeMillis() - created > UNANSWERED_MS
    }

    /**
     * Deals that will not move until this person does something.
     *
     * Two moments qualify, and only two: a deal waiting on money this device
     * owes, and a settlement the other side has proposed and parked. Both are
     * turns — the counterparty has done their part and is now waiting, in the
     * second case with their own money in an escrow that cannot pay out
     * without a second signature.
     *
     * Neither had anywhere to be seen. The notification fires once, and if it
     * is swiped away or arrives while the phone is face-down the app looks
     * exactly as it does when nothing is happening: the home screen shows a
     * balance and six tiles, and the person on the other side waits, believing
     * they have been ignored.
     *
     * The arbiter is excluded. It holds a share and no opinion (§9.3), and
     * nothing is waiting on it until it is asked.
     */
    fun waitingOnMe(context: Context): List<JSONObject> =
        all(context)
            .filter { it.optInt("kind") != KIND_BOND && !isArbiter(it) && !isFinished(it) }
            .filter { o ->
                when (o.optString("stage")) {
                    "done" -> {
                        val mine = mySharePxmr(o)
                        when {
                            mine <= 0 || myFundTxid(o).isNotEmpty() -> false
                            // The funder goes second wherever a host stake was
                            // asked for — the exposed side does not stand
                            // alone. Until that stake lands this is a wait,
                            // not a turn, and calling it one would nag
                            // somebody to pay for something not yet accepted.
                            isFunder(o) -> {
                                val hostDep = o.optLong("hostDepPxmr")
                                !(hostDep > 0 && o.optLong("fundedPxmr") < hostDep)
                            }
                            else -> true
                        }
                    }
                    // They proposed a split and it is parked on this device.
                    "release_pending" -> true
                    else -> false
                }
            }
            .sortedByDescending { it.optLong("created") }

    /** How long a turn goes unanswered before it is mentioned again. */
    private const val REMIND_AFTER_MS = 60L * 60 * 1000

    /**
     * Mention, once an hour, a turn nobody has taken.
     *
     * The proposal notification fires exactly once, when the message lands. A
     * phone that was face-down, or had notifications off, or whose owner swiped
     * it away on the way to something else, never mentions it again — and the
     * other side is left holding an escrow that cannot pay out, with no way to
     * tell being declined from being unseen.
     *
     * Hourly, and only while it is genuinely this device's turn: [waitingOnMe]
     * stops returning a deal the moment it is funded, signed, called off or
     * settled, so this stops with it. `nudgedAt` is per escrow rather than
     * global, so two waiting deals do not silence each other.
     */
    fun remindWaiting(context: Context) {
        val now = System.currentTimeMillis()
        val contacts = ContactStore(context).all()
        for (o in waitingOnMe(context)) {
            val idHex = o.optString("id")
            if (idHex.isEmpty()) continue
            val since = maxOf(o.optLong("nudgedAt"), o.optLong("created"))
            if (since <= 0 || now - since < REMIND_AFTER_MS) continue
            val peerHex = otherPrincipal(o) ?: continue
            val peer = contacts.firstOrNull { it.personaHex == peerHex } ?: continue
            Notify.post(
                context,
                peer.displayName(),
                context.getString(
                    if (o.optString("stage") == "release_pending") {
                        R.string.main_waiting_sign
                    } else {
                        R.string.main_waiting_pay
                    },
                    peer.displayName(),
                ),
                openChat = peerHex,
            )
            runCatching { mutate(context, idHex) { cur -> cur.put("nudgedAt", now) } }
        }
    }

    /** The txid this party would have written for its own funding, if any. */
    private fun myFundTxid(o: JSONObject): String =
        if (isFunder(o)) o.optString("fundTxid") else o.optString("hostFundTxid")

    fun rideWith(context: Context, peerHex: String): JSONObject? =
        all(context)
            .filter { it.optInt("kind") != KIND_BOND && !isArbiter(it) }
            .filter { otherPrincipal(it) == peerHex }
            // The newest one, whatever state it is in.
            //
            // This used to take the newest escrow that was still *live*, on
            // the reasoning that a released one is history. The effect was
            // that the instant a ride settled the banner fell back to
            // whatever unfinished escrow came before it — so a driver who had
            // just been paid was told, by the same screen, that they were
            // "waiting for the rider to secure the fare". An abandoned escrow
            // never expires, so this got worse the longer two people used the
            // app. A newer escrow supersedes an older one, full stop.
            .maxByOrNull { it.optLong("created") }
            ?.takeIf { bannerWorthy(it) }

    /** Whether this escrow still has something to say to the person. */
    private fun bannerWorthy(o: JSONObject): Boolean = when (o.optString("stage")) {
        "aborted" -> false
        // Settled: shown for a day so both sides get the same confirmation,
        // then it is history like any other finished thing.
        "released", "release_cosigned" -> {
            val at = o.optLong("settledAt")
            at > 0L && System.currentTimeMillis() / 1000 - at < SETTLED_SHOWN_SECS
        }
        else -> true
    }

    /**
     * No Monero node would answer.
     *
     * Typed because it is the one failure in here a person meets by having bad
     * signal rather than by something being wrong, and it was reaching them as
     * the four English words above — `moneyFailure` prints the message when it
     * recognises nothing, and it recognised nothing here. Everything else that
     * throws in this file is an invariant: a user who sees "the arbiter funds
     * nothing" has found a bug, and the sentence is for whoever reads the
     * report.
     */
    class NoNode : IllegalStateException("no node reachable")

    /**
     * This party has already funded this escrow.
     *
     * Also typed, and also not an invariant: two taps a second apart on a slow
     * network is the documented way to arrive here (see [fundRide]), and
     * paying twice into an address that needs a co-signature to give anything
     * back is the failure that costs. The second tap deserves a sentence in
     * the reader's own language saying their money is where they put it.
     */
    class AlreadyPaid : IllegalStateException("you have already paid into this escrow")

    /**
     * Call this deal off, and say so.
     *
     * `MessageKind::CeremonyAbort` has been in the wire format the whole time
     * — core validates it, refuses one that names no ceremony or carries a
     * round payload, and `ceremony_abort` clears the threshold machine it
     * names. Nothing in the app ever sent one. The result was that "aborted"
     * was a stage three screens could read and nothing could ever write, so a
     * proposal the far side did not want had no ending: no decline, no
     * cancel, and two phones each waiting for the other.
     *
     * Refused once there is money in it. An escrow either side has funded is
     * not called off, it is *released* — the funds need two signatures to move
     * and no local flag can conjure the second one. [isStale]'s test, for the
     * same reason and read from the same three places: this device's own scan
     * of the escrow address plus the two txids it would have written itself.
     *
     * The message is best-effort and the local record is not. A peer who is
     * offline learns about this on their next poll; a peer who never comes
     * back leaves an escrow that goes stale on its own, which is what
     * [isStale] is for. What must not happen is this device staying on the
     * hook because the network was down when somebody said no.
     */
    fun callOff(context: Context, idHex: String) {
        val o = load(context, idHex) ?: throw IllegalStateException("no such ceremony")
        check(!isFinished(o)) { "this escrow is already over" }
        check(o.optLong("fundedPxmr") == 0L) { "there is money in this escrow" }
        check(o.optString("fundTxid").isEmpty()) { "there is money in this escrow" }
        check(o.optString("hostFundTxid").isEmpty()) { "there is money in this escrow" }

        val id = hexToBytes(o.optString("id")) ?: throw IllegalStateException("no ceremony id")
        val mineHex = PersonaStore(context).personaHex()
        val roster = o.optJSONArray("roster")?.let { arr ->
            (0 until arr.length()).map { arr.getString(it) }
        } ?: emptyList()
        for (peerHex in roster.filter { it != mineHex }) {
            val peer = contactFor(context, peerHex) ?: continue
            runCatching {
                // A body, because core refuses a message of zero
                // characters and this one carries its meaning in its kind.
                // Never read by anybody: kind 10 is filtered out of the
                // thread like the other ceremony traffic, and the reader's
                // own phone writes the sentence they see.
                Mailbox.send(
                    context, peer, "ceremony: called off", mineHex,
                    kind = 10, ceremonyId = id,
                )
            }.onFailure { DucatLog.w(TAG, "abort $idHex: ${it.message}") }
        }
        runCatching { uniffi.ducat_mobile.ceremonyAbort(id, o.optInt("i").toUShort()) }
        mutate(context, idHex) { cur -> settle(cur, "aborted") }
        ContactStore.bump()
        DucatLog.i(TAG, "escrow $idHex: called off")
    }

    /**
     * The other side called it off.
     *
     * Only ever *toward* aborted, and only from a state with nothing at stake.
     * A peer cannot end an escrow this device has money in by sending a
     * message — that is what the two signatures are for — so a stray or
     * malicious abort against a funded escrow is dropped rather than obeyed.
     */
    fun onAbort(context: Context, idHex: String) {
        val o = load(context, idHex) ?: return
        if (isFinished(o)) return
        if (o.optLong("fundedPxmr") > 0 ||
            o.optString("fundTxid").isNotEmpty() ||
            o.optString("hostFundTxid").isNotEmpty()
        ) {
            DucatLog.w(TAG, "escrow $idHex: abort ignored — it holds money")
            return
        }
        hexToBytes(o.optString("id"))?.let {
            runCatching { uniffi.ducat_mobile.ceremonyAbort(it, o.optInt("i").toUShort()) }
        }
        mutate(context, idHex) { cur -> settle(cur, "aborted") }
        ContactStore.bump()
        DucatLog.i(TAG, "escrow $idHex: the other side called it off")
    }

    /** Stamp the moment an escrow stopped needing anyone. */
    private fun settle(o: JSONObject, stage: String): JSONObject =
        o.put("stage", stage).put("settledAt", System.currentTimeMillis() / 1000)

    /** The rider pays the fare into the escrow — an ordinary wallet send to
     *  an address that happens to need two of three keys to leave. */
    fun fundRide(context: Context, idHex: String): String {
        val o = load(context, idHex) ?: throw IllegalStateException("no such ceremony")
        check(o.optString("stage") == "done") { "the escrow is not built yet" }
        // Whoever owes the escrow something may pay it, and nobody else:
        // the rider's fare, the driver's stake on a two-sided ride, each
        // principal's own share of a reservation — where the host funding
        // theirs IS the acceptance.
        check(!isArbiter(o)) { "the arbiter funds nothing" }
        val addr = o.optString("address")
        val share = mySharePxmr(o)
        check(addr.isNotEmpty()) { "the escrow has no address yet" }
        check(share > 0) { "you owe this escrow nothing" }
        // Paid already, by this party. The banner hides the button once a
        // send is recorded, but a second tap on a slow network — or a client
        // restarted mid-flight — must not pay twice into an escrow that
        // needs a co-signature to give anything back.
        val already = if (isFunder(o)) o.optString("fundTxid") else o.optString("hostFundTxid")
        if (already.isNotEmpty()) throw AlreadyPaid()
        val nodeUrl = node(context) ?: throw NoNode()
        val mine = if (isFunder(o)) "fundTxid" else "hostFundTxid"
        // Claim the slot *before* the money moves, on the record as it stands
        // now. Two taps a second apart both passed the check above and both
        // reached the send, because the mark that stops the second one was
        // only written after the first had finished — and a transaction takes
        // seconds to build. Writing "sending" here makes the second tap fail
        // the same check the first passed.
        //
        // A claim left behind by a process that died mid-send would otherwise
        // lock this party out of their own escrow for good, so it is checked
        // against the wallet rather than trusted or timed out. The wallet
        // records what it sent and where; a claim with no payment to this
        // address behind it is debris and may be taken again. A claim *with*
        // one is a payment that landed and was never written down — recorded
        // now, and refused, because paying twice is the failure that costs.
        mutate(context, idHex) { cur ->
            val held = cur.optString(mine)
            if (held == SENDING) {
                val sent = WalletStore(context).sends().firstOrNull { it.toAddress == addr }
                if (sent != null) {
                    cur.put(mine, sent.txidHex)
                    DucatLog.w(TAG, "escrow $idHex: recovered a send nobody recorded")
                    throw AlreadyPaid()
                }
                DucatLog.i(TAG, "escrow $idHex: retaking a claim left by a send that never went")
            } else {
                if (held.isNotEmpty()) throw AlreadyPaid()
            }
            cur.put(mine, SENDING)
        }
        // One send for this party's whole share: the deposits come home in
        // the split release, and are what make releasing beat sulking.
        val r = runCatching { Wallet.send(context, nodeUrl, addr, share) }
            .onFailure {
                // Nothing left, so nothing is owed to the mark. Release it, or
                // a node that timed out locks this party out of their own
                // escrow for good.
                runCatching { mutate(context, idHex) { cur -> cur.put(mine, "") } }
            }
            .getOrThrow()
        // Per-party mark: for reservations both sides fund, each records
        // their own send. On a record loaded *now*, not on the snapshot this
        // function started with — rounds arrive on the poller while a
        // transaction is being built, and writing the old object back put the
        // ceremony into a state the protocol had already left.
        mutate(context, idHex) { cur -> cur.put(mine, r.txidHex) }
        ContactStore.bump()
        DucatLog.i(TAG, "escrow $idHex: ${formatXmr(share)} XMR sent — ${r.txidHex.take(16)}…")
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
     * The driver marks the ride complete: the default proposal, giving the
     * rider back exactly their margin and the fare to the driver.
     */
    fun proposeRideRelease(context: Context, idHex: String): Long {
        val o = load(context, idHex) ?: throw IllegalStateException("no such ceremony")
        val keys = hexToBytes(o.optString("keys"))
            ?: throw IllegalStateException("this device holds no key share")
        val nodeUrl = node(context) ?: throw NoNode()
        val from = o.optLong("scanFrom").takeIf { it > 0 }
            ?: WalletStore(context).restoreHeight().toLong()
        val total = runCatching {
            uniffi.ducat_mobile.escrowBalance(keys, nodeUrl, from.toULong()).toLong()
        }.getOrDefault(0L)
        // What comes home to the funder, and only that.
        //
        // "Everything above the fare" was right while the rider was the only
        // one paying in. It stopped being right the day the driver staked
        // too: the pot then holds fare + rider stake + driver stake, and
        // handing back everything above the fare would give the rider the
        // driver's stake as well — a successful ride that quietly robs the
        // driver. The funder's own stake is the number, recorded in the
        // ceremony at birth so a later change to the suggested percentage
        // cannot re-price an escrow that is already standing.
        val recorded = o.optLong("funderDepPxmr")
        val back = when {
            o.optInt("kind") == KIND_RESERVATION -> recorded.coerceAtMost(total)
            recorded > 0 -> recorded.coerceAtMost(total)
            // Ceremonies built before the stake was recorded: derive it from
            // the pot, less the fare and less whatever the driver staked.
            else -> (total - o.optLong("farePxmr") - o.optLong("hostDepPxmr"))
                .coerceAtLeast(0L)
                .coerceAtMost((total - o.optLong("farePxmr")).coerceAtLeast(0L))
        }
        return proposeRideSplit(context, idHex, back)
    }

    /**
     * Propose any split of the escrow: `riderBackPxmr` home to the funder's
     * refund address, the rest (minus the true network fee) to the driver.
     *
     * **Either principal may propose** — this is the settlement screen's
     * engine (§15.12): a counter-offer is just a fresh proposal, and it
     * supersedes whatever was parked, including the proposer's own earlier
     * one. Whoever signs first ends the negotiation; the burn is only what
     * happens if nobody ever does. The proposal message carries the claimed
     * number so the other side's consent screen can state it — the payload
     * is signed, the statement is beside it.
     *
     * When nearly everything goes back to the rider, the roles in the
     * transaction flip: the refund address becomes the residual claimant
     * (and pays the fee), because a residual too small to cover the fee is
     * not a transaction.
     */
    fun proposeRideSplit(
        context: Context,
        idHex: String,
        riderBackPxmr: Long,
        /** §9.3: the counterparty is gone, so this proposal goes to the
         *  arbiter instead — their co-signature IS the ruling. The split's
         *  destinations do not change; only who is asked to agree does. */
        toArbiter: Boolean = false,
    ): Long {
        val o = load(context, idHex) ?: throw IllegalStateException("no such ceremony")
        // done → first proposal; releasing → retry or self-supersede;
        // release_pending / release_cosigned → the counter-offer. Only
        // "released" is final: broadcast money does not renegotiate.
        check(o.optString("stage") in
            listOf("done", "releasing", "release_pending", "release_cosigned")) {
            "the escrow is not ready to release"
        }
        check(!isArbiter(o)) { "the arbiter holds a share, not an opinion" }
        val id = hexToBytes(idHex)!!
        val i = o.optInt("i")
        val keys = hexToBytes(o.optString("keys"))
            ?: throw IllegalStateException("this device holds no key share")
        val nodeUrl = node(context) ?: throw NoNode()
        val from = o.optLong("scanFrom").takeIf { it > 0 }
            ?: WalletStore(context).restoreHeight().toLong()
        val roster0 = o.getJSONArray("roster").let { arr ->
            (0 until arr.length()).map { arr.getString(it) }
        }
        val peerHex = if (toArbiter) {
            val arb = o.optInt("arbiterIdx")
            check(arb != 0) { "this escrow has no arbiter — only the counterparty can sign" }
            roster0[arb - 1]
        } else {
            otherPrincipal(o) ?: throw IllegalStateException("no counterparty")
        }
        val peer = contactFor(context, peerHex)
            ?: throw IllegalStateException("the counterparty is not a contact")
        val refund = o.optString("refundAddr")
        check(refund.isNotEmpty()) { "this ceremony has no refund address" }
        // The driver's payout address: their own wallet when the driver
        // proposes; when the rider proposes, the driver's published
        // subaddress from the handshake — a rider cannot route the fare
        // anywhere the driver did not name. Always the DRIVER's address,
        // whoever the proposal is being sent to.
        val driverDest = if (isFunder(o)) {
            val driverHex = otherPrincipal(o)
                ?: throw IllegalStateException("no counterparty")
            contactFor(context, driverHex)?.theirAddress
                ?: throw IllegalStateException(
                    "the driver has not published an address — ask them to propose instead")
        } else {
            WalletStore(context).address()
                ?: throw IllegalStateException("no wallet to receive the fare")
        }

        val total = runCatching {
            uniffi.ducat_mobile.escrowBalance(keys, nodeUrl, from.toULong()).toLong()
        }.getOrDefault(0L)
        val back = riderBackPxmr.coerceIn(0L, total)
        // Two shapes, one meaning: normally the rider's slice is fixed and
        // the driver is residual; when the driver's remainder could not
        // cover the fee, the driver's slice is fixed (possibly zero) and
        // the rider is residual.
        // ...and flipping only helps if the side that *becomes* residual has
        // a share to take the fee from. It did not check that. On an ordinary
        // sale nothing goes back to the buyer, so the whole escrow is the
        // seller's — `total - back` is everything, which is nevertheless
        // "under two fee reserves" on a small item, so it flipped: every
        // piconero into a fixed output, and a residual side holding zero to
        // pay a fee out of. The core then refused the release with "the
        // escrow cannot cover the split and the fee", with the money already
        // locked in it. Found by selling a coffee grinder for twelve cents.
        val margin = MIN_ESCROW_PXMR
        val flip = total - back < margin && back >= margin
        val prop = if (!flip) {
            uniffi.ducat_mobile.frostProposeSplit(
                id, i.toUShort(), keys,
                if (back > 0) listOf(uniffi.ducat_mobile.SplitOut(refund, back.toULong()))
                else emptyList(),
                driverDest, nodeUrl, from.toULong(),
            )
        } else {
            uniffi.ducat_mobile.frostProposeSplit(
                id, i.toUShort(), keys,
                if (total - back > 0)
                    listOf(uniffi.ducat_mobile.SplitOut(driverDest, (total - back).toULong()))
                else emptyList(),
                refund, nodeUrl, from.toULong(),
            )
        }
        Mailbox.send(
            context, peer,
            if (toArbiter) "ride: asking the arbiter to rule" else "ride: proposed a split",
            PersonaStore(context).personaHex(),
            kind = 9, round = 0, ceremonyId = id, payload = prop.payload,
            amountPxmr = back,
        )
        o.put("stage", "releasing")
        o.put("cosignerIdx", indexOf(
            o.getJSONArray("roster").let { arr -> (0 until arr.length()).map { arr.getString(it) } },
            peerHex,
        ))
        o.put("myRiderBack", back)
        o.put("payoutPxmr", prop.payoutPxmr.toLong())
        save(context, idHex, o)
        ContactStore.bump()
        DucatLog.i(TAG, "ride $idHex: proposed split — $back pXMR back to the rider")
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
        settle(o, "release_cosigned")
        o.remove("pendingPayload")
        save(context, idHex, o)
        ContactStore.bump()
        DucatLog.i(TAG, "ride $idHex: rider approved the release (fee ${ans.feePxmr})")
    }
}
