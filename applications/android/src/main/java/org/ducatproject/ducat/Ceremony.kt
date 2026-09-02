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
    private fun save(
        context: Context,
        id: String,
        o: JSONObject,
        /** True for a claim that must be on disk BEFORE an irreversible
         *  action runs (a broadcast, a spend). apply() hands the write to a
         *  background queue a process death skips, which un-writes exactly
         *  the marker the recovery path would have read. */
        durable: Boolean = false,
    ) = synchronized(lock) {
        val merged = mergeOnto(loadedAs[o], prefs(context).getString("c_$id", null), o)
        if (merged !== o) DucatLog.i(TAG, "ceremony $id: merged onto a record that moved")
        val text = merged.toString()
        val e = prefs(context).edit().putString("c_$id", text)
        if (durable) e.commit() else e.apply()
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
    /**
     * Every ceremony this device knows about.
     *
     * Read out of the one decrypted snapshot rather than decrypted twice. The
     * store is encrypted at rest, so `.all` already costs a pass over every
     * entry — and this then asked for each one again by key, and re-resolved
     * `prefs(context)` inside the loop while doing it. Three screens call this
     * on every change to the contact store, one of them the home screen, so
     * the wasted work landed on the main thread every time a poll ticked.
     *
     * A record that will not parse is skipped rather than thrown: this is on
     * the path that draws the home screen, and one malformed entry taking the
     * whole screen down is a worse answer than one row missing.
     */
    fun all(context: Context): List<JSONObject> =
        prefs(context).all
            .asSequence()
            .filter { it.key.startsWith("c_") }
            .mapNotNull { (it.value as? String) }
            .mapNotNull { runCatching { JSONObject(it) }.getOrNull() }
            .toList()

    /**
     * The open escrows, as §4.3.3 shares for the backup.
     *
     * These live in their own store, not the contact one the rest of the
     * bundle is assembled from, which is how they came to be left out of it
     * entirely — the backup screen has been telling people an open escrow
     * needs a fresher bundle while the export threw the shares away.
     *
     * Finished ones are skipped: a released escrow restores as a released
     * escrow, and carrying spent key material forward is a cost with no
     * recipient. What travels is the whole record, because the share alone is
     * not a resumable escrow — which roster, which index, which stage, and how
     * far to scan are all in here too, and a key package without them restores
     * something nobody can act on.
     */
    fun backupShares(context: Context): List<uniffi.ducat_mobile.EscrowShareEntry> =
        all(context)
            .asSequence()
            .filterNot { isFinished(it) }
            .mapNotNull { o ->
                val id = hexToBytes(o.optString("id")) ?: return@mapNotNull null
                // No key package yet means the DKG never finished, so there is
                // nothing to sign with and nothing at stake to protect.
                if (o.optString("keys").isEmpty()) return@mapNotNull null
                uniffi.ducat_mobile.EscrowShareEntry(
                    escrowId = id,
                    share = o.toString().toByteArray(),
                    restoreHeight = o.optLong("scanFrom").coerceAtLeast(0L).toULong(),
                )
            }
            .toList()

    /**
     * Put restored escrows back, and never over one already here.
     *
     * A bundle's copy of an escrow is a photograph of it; the record on disk is
     * the escrow. If both exist, the disk one has been carried forward by this
     * device's own participation and is strictly the newer of the two — laying
     * a snapshot over it would rewind a stage, and a stage rewound here is a
     * ceremony that stalls or a funding mark that lets a second payment go.
     *
     * So: only what is missing. That is also the only case that matters, since
     * the path this exists for is a device with nothing on it at all.
     */
    fun restoreShares(context: Context, shares: List<uniffi.ducat_mobile.EscrowShareEntry>) {
        var added = 0
        var kept = 0
        for (s in shares) {
            val o = runCatching { JSONObject(String(s.share)) }.getOrNull() ?: continue
            val id = o.optString("id").ifEmpty { s.escrowId.toHexString() }
            synchronized(lock) {
                if (prefs(context).getString("c_$id", null) != null) {
                    kept += 1
                } else {
                    prefs(context).edit().putString("c_$id", o.toString()).apply()
                    added += 1
                }
            }
        }
        if (added > 0 || kept > 0) {
            DucatLog.i(TAG, "escrows from the backup: $added restored, $kept already here")
        }
    }

    /** How long a finished escrow stays readable before it is forgotten. */
    private const val KEEP_FINISHED_MS = 7L * 24 * 60 * 60 * 1000

    /** Concurrent unfunded deals one contact may have open with you. */
    private const val OPEN_PER_CONTACT = 3

    /**
     * Forget escrows that are over, and ones nobody ever answered.
     *
     * §18.7's stewardship, applied to the one store that had no sweep. Every
     * round-0 that arrives writes a record here and none was ever removed —
     * `isFinished` existed and nothing called it — so the set only grew, and
     * three screens read all of it on every poll tick, one of them Home.
     *
     * Nothing with money in it is touched. `isStale` already carries that
     * argument: it refuses any escrow this device has seen funded, by its own
     * scan and by the two txids it would have written itself, precisely
     * because the way out of a funded escrow is a release or an arbiter and
     * not a screen quietly deciding it is over. Finished ones wait a week
     * first, so a deal that just settled is still there to look at.
     */
    fun sweep(context: Context): Int {
        val now = System.currentTimeMillis()
        val doomed = all(context).filter { o ->
            // Never a record holding a key share. This is the whole safety
            // rule and it sits above every other test on purpose.
            //
            // A share means the ceremony finished and an address exists on a
            // chain anybody can pay. Money can arrive at it tomorrow, and the
            // share is this device's only means of ever moving that money —
            // there is no second copy and no way to derive it again.
            //
            // The tests below cannot stand in for this. Every one of them
            // asks whether *this* device has seen funding, and there are
            // parties who never will: an arbiter neither funds nor is offered
            // the banner that scans (checkRideFunding has exactly one caller,
            // Chat.kt, inside the ride banner the arbiter is excluded from),
            // and a one-sided ride's driver or a zero-deposit host stake
            // nothing by design. For all of them the funding tests are
            // permanently false, so every 2-of-3 quietly became a 2-of-2
            // half an hour after it was built — losing the exact property
            // the escrow is sold on.
            //
            // The cost of keeping them is a few hundred bytes per completed
            // escrow, for ever. That is the right side to err on.
            if (holdsShare(o)) return@filter false
            when {
                isStale(o) -> true
                isFinished(o) -> {
                    val at = o.optLong("created").takeIf { it > 0 } ?: return@filter false
                    Elapsed.due(now, at, KEEP_FINISHED_MS)
                }
                else -> false
            }
        }.mapNotNull { it.optString("id").takeIf { s -> s.isNotEmpty() } }
        if (doomed.isEmpty()) return 0
        synchronized(lock) {
            val e = prefs(context).edit()
            doomed.forEach { e.remove("c_$it") }
            e.apply()
        }
        DucatLog.i(TAG, "swept ${doomed.size} finished or unanswered escrow(s)")
        return doomed.size
    }

    /**
     * Whether this contact may open another deal with us right now.
     *
     * A round-0 is joined without asking: it commits, messages the rest of the
     * roster and writes a record that used to live for ever. Unbounded, that is
     * a contact who can make this phone cut prekeys and write to the DHT for as
     * long as they care to. Nothing here moves money, so the answer is a
     * ceiling rather than a consent screen.
     *
     * Only *unfunded* ones count. An escrow with money in it is a real
     * obligation and must never be refused a round because of its neighbours.
     */
    private fun tooManyOpen(context: Context, peerHex: String): Boolean =
        all(context).count { o ->
            o.optString("peer") == peerHex &&
                !isFinished(o) &&
                !isStale(o) &&
                o.optLong("fundedPxmr") == 0L &&
                o.optString("fundTxid").isEmpty() &&
                o.optString("hostFundTxid").isEmpty()
        } >= OPEN_PER_CONTACT

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

    internal fun frameRound0(
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

    internal data class Invite(
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

    internal fun parseRound0(payload: ByteArray): Invite? {
        var p = 0
        // `n < 0` as well as past-the-end, because this parser's whole contract
        // is that malformed input comes back as null. A length byte is written
        // unsigned (`out.write(size)` is the low eight bits) and was read with
        // `Byte.toInt()`, which **sign-extends** — so any length from 128 up
        // arrived as negative, `copyOfRange(p, p - k)` threw
        // IllegalArgumentException, and the one path documented to return null
        // instead raised. The caller's runCatching contained it, so this cost a
        // confusing log line rather than a crash, but "returns null when
        // malformed" was not true and the next caller might not have caught.
        fun take(n: Int): ByteArray? {
            if (n < 0 || p + n > payload.size) return null
            return payload.copyOfRange(p, p + n).also { p += n }
        }
        /** A byte off the wire, read the way it was written: unsigned. */
        fun byte(): Int? = take(1)?.get(0)?.toInt()?.and(0xFF)
        val n = byte() ?: return null
        if (n < 2 || n > 3) return null
        val roster = (0 until n).map { take(32)?.toHexString() ?: return null }
        val arbiterIdx = byte() ?: return null
        val kind = byte() ?: return null
        val funderIdx = byte() ?: return null
        val fare = take(8)?.let {
            java.nio.ByteBuffer.wrap(it).order(java.nio.ByteOrder.LITTLE_ENDIAN).long
        } ?: return null
        val refundLen = byte() ?: return null
        val refund = String(take(refundLen) ?: return null)
        fun u64(): Long? = take(8)?.let {
            java.nio.ByteBuffer.wrap(it).order(java.nio.ByteOrder.LITTLE_ENDIAN).long
        }
        val fDep = u64() ?: return null
        val hDep = u64() ?: return null
        val nonceLen = byte() ?: return null
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
                kind = 8, round = 0, ceremonyId = id, payload = frame,
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
                // And *which* listing, for the same reason the title is here:
                // the thread's subject moves on, so the only honest moment to
                // ask is when the deal is struck. Empty on the buyer's side —
                // they know the notice, not the record behind it — which is
                // exactly what makes this safe to act on: only the owner of a
                // listing can match it (see [soldOne]).
                put("aboutListing", a.listingId)
            }
            put("i", i); put("stage", "committed")
            put("commits", JSONObject()); put("shares", JSONObject())
            // The bytes, not the recipe. dkgCommit draws from OsRng and
            // replaces the engine's machine, so asking for the commitment a
            // second time produces a different one and destroys the first —
            // a retransmit has to be the same bytes or it is a fork. See
            // [nudge].
            put("sent0", frame.toHexString())
            put("progressAt", System.currentTimeMillis())
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
            // And the one inviting must be on it as well. Anyone this device
            // has a thread with could otherwise hand it a roster of two other
            // people and have it commit to — and later count itself into —
            // a deal it was never party to, with [senderIdx] below reading 0
            // for every frame that followed.
            if (contact.personaHex !in inv.roster) {
                DucatLog.w(TAG, "bond $idHex: the invite came from outside its own roster")
                return
            }
            // A ceiling on how many unfunded deals one contact may have open
            // with us at once. Joining is automatic and free to ask for.
            if (tooManyOpen(context, contact.personaHex)) {
                DucatLog.w(
                    TAG,
                    "bond $idHex: ${contact.displayName()} already has " +
                        "$OPEN_PER_CONTACT unfunded deals open — not joining",
                )
                return
            }
            if (!ceremonyId(inv.roster, inv.nonce).contentEquals(id)) {
                DucatLog.w(TAG, "bond $idHex: roster does not hash to the ceremony id")
                return
            }
            val i = indexOf(inv.roster, mineHex)
            val n = inv.roster.size
            // **Our own money comes home to our own address.**
            //
            // Every economic term in a round-0 frame is the inviter's word for
            // it, and the ceremony id binds only the roster and the nonce — so
            // the id check above proves the frame is self-consistent, never
            // that it is what the two of us agreed. An honest client always
            // names *itself* the funder (see start), so this is invisible in
            // normal use. A modified one names the victim as funder and keeps
            // its own address in refundAddr.
            //
            // refundAddr is the funder's residual destination in every split
            // proposeRideSplit can build — the ordinary one, the counter, and
            // the ask-the-arbiter one. Adopted verbatim, there was no split
            // the funder could construct that returned their own money.
            //
            // So the funder mints it locally, from the same seed start uses,
            // and ignores what was sent. The frame echoed back to the roster
            // still carries the inviter's value: the echo is the ceremony's
            // self-description and every copy has to agree on it, and what
            // this device *spends to* is its own record, not the echo.
            val invitedRefund = inv.refundAddr
            val myRefund = if (i == inv.funderIdx && inv.kind != KIND_BOND) {
                WalletStore(context).addressFor("ride_$idHex") ?: invitedRefund
            } else invitedRefund
            if (myRefund != invitedRefund) {
                DucatLog.w(
                    TAG,
                    "bond $idHex: the invite named this device the funder and " +
                        "supplied someone else's refund address — using our own",
                )
            }
            val commit =
                uniffi.ducat_mobile.dkgCommit(id, i.toUShort(), T.toUShort(), n.toUShort())
            // The join echoes the invite's own kind/funder/fare — the frame
            // is the ceremony's self-description and every copy must agree.
            val frame = frameRound0(
                inv.roster, inv.arbiterIdx, inv.kind, inv.funderIdx, inv.farePxmr,
                inv.refundAddr, inv.funderDepPxmr, inv.hostDepPxmr, inv.nonce, commit,
            )
            // Everyone on the roster has to be someone this device can write
            // to, or the join is a commitment nobody will ever receive.
            val peers = inv.roster.filter { it != mineHex }.map { peerHex ->
                contactFor(context, peerHex) ?: run {
                    DucatLog.w(TAG, "bond $idHex: ${peerHex.take(8)}… is not my contact — cannot join")
                    return
                }
            }
            o = JSONObject().apply {
                put("id", idHex); put("nonce", inv.nonce)
                put("roster", JSONArray(inv.roster)); put("arbiterIdx", inv.arbiterIdx)
                put("kind", inv.kind); put("funderIdx", inv.funderIdx)
                put("farePxmr", inv.farePxmr)
                put("refundAddr", myRefund)
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
                    // And here above all: the paragraph above says the host is
                    // the party who always joins, and the host is the one
                    // holding the listing. Recorded on the start path too, but
                    // that side is the buyer, whose About carries no listing
                    // id — so [soldOne] would have had nothing to work with if
                    // this were only written over there.
                    put("aboutListing", a.listingId)
                }
                put("i", i); put("stage", "committed")
                put("commits", JSONObject()); put("shares", JSONObject())
                put("sent0", frame.toHexString())
                put("progressAt", System.currentTimeMillis())
            }
            // Written down before a single frame leaves. dkgCommit has
            // already made the machine, and the commitment it produced is
            // in `sent0`: a send that fails (or a process that dies) between
            // here and the last peer used to leave no record at all, so the
            // next round-0 from the inviter was treated as a fresh invite
            // and committed *again* — a second machine for the same
            // ceremony, and a roster holding two different commitments from
            // this device. Saved first, the retry is [resend]'s job.
            save(context, idHex, o)
            var sent = 0
            for (peer in peers) {
                runCatching {
                    Mailbox.send(
                        context, peer, "bond: building a shared deposit",
                        kind = 8, round = 0, ceremonyId = id, payload = frame,
                    )
                    sent++
                }.onFailure {
                    DucatLog.w(
                        TAG,
                        "bond $idHex: commitment to ${peer.displayName()} did not go: ${it.message}",
                    )
                }
            }
            DucatLog.i(TAG, "joined bond $idHex (i=$i of $n), sent commitment to $sent of ${peers.size}")
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
            // What they say they formed. Compared, never adopted — a
            // disagreement is the finding, so taking their answer would
            // destroy the evidence.
            2 -> {
                val theirs = runCatching { String(payload, Charsets.UTF_8) }.getOrNull()
                if (!theirs.isNullOrBlank()) {
                    o.put(
                        "addrs",
                        (o.optJSONObject("addrs") ?: JSONObject())
                            .put(senderIdx.toString(), theirs),
                    )
                }
            }
        }
        // Something arrived from somebody: the build is moving, so the
        // retransmit clock in [nudge] starts again from here.
        o.put("progressAt", System.currentTimeMillis())
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
                // Kept before any send is tried: dkgShare consumes the
                // round-1 machine, so these bytes cannot be asked for twice.
                // A send that threw used to unwind past the save — the
                // shares were gone with the machine, the stage still read
                // "committed", and every later frame found nothing to
                // advance. A peer who is briefly unreachable gets theirs
                // from [resend] instead.
                val sent1 = JSONObject()
                for (s in shares) {
                    sent1.put(s.participant.toInt().toString(), s.bytes.toHexString())
                }
                o.put("sent1", sent1)
                o.put("stage", "shared"); save(context, idHex, o)
                var sent = 0
                for (s in shares) {
                    val peerHex = roster[s.participant.toInt() - 1]
                    val peer = contactFor(context, peerHex) ?: continue
                    runCatching {
                        Mailbox.send(
                            context, peer, "bond: your share",
                            kind = 8, round = 1, ceremonyId = id, payload = s.bytes,
                        )
                        sent++
                    }.onFailure {
                        DucatLog.w(
                            TAG,
                            "bond $idHex: share to ${peer.displayName()} did not go: ${it.message}",
                        )
                    }
                }
                DucatLog.i(TAG, "bond $idHex: shared, sent $sent of ${shares.size} share(s)")
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
                // On disk now, not after the announcements below. dkgTakeKeys
                // hands the share out once; it is the only copy of this
                // device's part of the escrow key, and what follows is a
                // blocking send per peer and a node pick — long enough for
                // the process to be reclaimed mid-way, which would have left
                // a "shared" record with n-1 shares and no key to spend with.
                save(context, idHex, o)
                // **Say which wallet we formed, and wait to hear the same back.**
                //
                // Round 0's commitments travel pairwise — there is no broadcast
                // channel here and no echo round — so a participant can send
                // one commitment to B and a different one to C. Both verify:
                // each is self-consistent and carries a valid proof of
                // possession. B and C then derive *different group keys*, and
                // therefore different escrow addresses, and nothing compared
                // them. The funder funds B's address while the arbiter holds a
                // share of C's, which is a 2-of-2 with the attacker wearing
                // the shape of a 2-of-3.
                //
                // core::escrow::check_escrow_ready has made exactly this
                // comparison since it was written — "three parties can each
                // complete a ceremony successfully and end up in different
                // groups" — and it was never reachable, because it needs three
                // reports and nothing ever exchanged them. This is that
                // exchange, as a third DKG round.
                o.put("addrs", (o.optJSONObject("addrs") ?: JSONObject())
                    .put(i.toString(), addr))
                for (peerHex in roster.filter { it != mineHex }) {
                    val peer = contactFor(context, peerHex) ?: continue
                    runCatching {
                        Mailbox.send(
                            context, peer, "bond: the wallet I formed",
                            kind = 8, round = 2, ceremonyId = id,
                            payload = addr.toByteArray(),
                        )
                    }.onFailure {
                        DucatLog.w(TAG, "bond $idHex: could not announce the address: ${it.message}")
                    }
                }
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
                }.onFailure {
                    // Not fatal and not free: with no height here every later
                    // scan of this escrow falls back to the wallet's restore
                    // height and reads the chain from there — minutes where
                    // this would have been seconds, for the life of the
                    // escrow, with nothing anywhere to say why. Now a line.
                    DucatLog.w(
                        TAG,
                        "bond $idHex: no scan height (${it.message}) — scans start from the wallet's",
                    )
                }
                save(context, idHex, o)
                ContactStore.bump()
                DucatLog.i(TAG, "bond $idHex done — escrow $addr")
            } else if (o.optString("stage") == "shared") {
                DucatLog.i(TAG, "bond $idHex: share ${sh.length()}/${n - 1}")
            }

            // **A finished party still owes the unfinished one its bytes.**
            //
            // [nudge] only fires while this device is itself half-built, and
            // the device that is stuck is rarely the one that can unstick
            // itself: the party that lost a frame needs it re-sent by the
            // party that sent it, who by then has everything and has stopped
            // asking for anything. An early round arriving here after we are
            // done is that party knocking — it can only have come from a
            // retransmit — so answer it with everything we ever sent them.
            //
            // Never at somebody who has already finished, and never twice in
            // a hurry. Answering unconditionally is a live-lock between two
            // *completed* parties: each reads the other's catch-up as a party
            // in trouble and catches it up straight back, every two seconds,
            // for as long as both stay up (seen on the first run of this
            // code). A round-2 address echo from a participant is that
            // participant saying they are done, so it is also the signal to
            // stop helping them; the interval is the belt to that's braces,
            // and bounds the exchange even if the echo never arrived.
            val theyFinished = o.optJSONObject("addrs")?.has(senderIdx.toString()) == true
            val caughtUp = o.optJSONObject("caughtUp") ?: JSONObject()
            val lastHelp = caughtUp.optLong(senderIdx.toString())
            if (o.optString("stage") == "done" && round.toInt() <= 1 &&
                !theyFinished &&
                Elapsed.due(System.currentTimeMillis(), lastHelp, NUDGE_AFTER_MS)
            ) {
                caughtUp.put(senderIdx.toString(), System.currentTimeMillis())
                o.put("caughtUp", caughtUp)
                save(context, idHex, o)
                runCatching { resend(context, o, senderIdx) }
                    .onFailure { DucatLog.w(TAG, "bond $idHex: catch-up — ${it.message}") }
            }
        }.onFailure {
            DucatLog.w(TAG, "bond $idHex round $round failed: ${it.message}")
            // Not aborted. The machine that failed to advance is still in
            // memory (the engine parses a peer's bytes before it takes the
            // machine out, and puts it back on refusal), and every other
            // failure here — a send, a corrupt bucket — is one a later frame
            // or a retransmit can still get past. Abort made all of them
            // final: it threw the machine away, and §17.9 says nothing can
            // rebuild one.
            //
            // **A build this device can no longer finish, written down.**
            //
            // The DKG machine lives in memory (§17.9), so a phone that dies
            // between the first frame and the last takes it with it — and
            // every frame that arrives afterwards is refused with "no dkg in
            // progress for this ceremony". That is terminal: `nudge` can
            // replay our own stored bytes, but nothing can rebuild a machine
            // that is gone, so the ceremony will never advance on this side.
            //
            // The screen did not know. It went on saying "Securing … —
            // building the escrow…" for as long as anyone looked at it, a
            // spinner for something already over, with the retransmit failing
            // the same way every three minutes underneath. Recorded here so
            // the banner can say what has happened and point at the way out.
            //
            // Only while it is still building: the same refusal is the
            // ordinary answer to a late duplicate frame for a ceremony that
            // finished, which is nothing to report.
            if (it.message?.contains("no dkg in progress") == true &&
                o.optString("stage") !in setOf("done", "release_pending", "releasing")
            ) {
                mutate(context, idHex) { cur -> cur.put("lostMachine", true) }
                DucatLog.w(TAG, "bond $idHex: this device cannot finish the build")
            }
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
        // Bonds only, and never through an arbiter's record. Unfiltered,
        // this picked ANY done ceremony with the peer — an arbiter shown
        // "Return the deposit" over a live ride escrow would propose a full
        // sweep of the pot to its own wallet, and a principal could clobber
        // a ride's stage the same way. Newest by created, because
        // prefs.all's iteration order is nobody's promise.
        val idHex = all(context)
            .filter {
                it.optString("peer") == contact.personaHex &&
                    it.optInt("kind") == KIND_BOND &&
                    !isArbiter(it)
            }
            .filter { it.optString("stage") == "done" }
            .maxByOrNull { it.optLong("created") }
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
            kind = 9, round = 0, ceremonyId = id, payload = prop.payload,
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
                // Bonds included in the mid-release stages: two phones that
                // both tapped "Return the deposit" are both at "releasing",
                // and with bonds carved out here each side threw the other's
                // proposal away — a polite deadlock nothing on either screen
                // could break. A bond proposal parks at release_pending like
                // any other and waits for the PIN, so accepting it costs
                // nothing a person did not sign for; whoever signs first
                // ends it, which is the counter-offer rule already below.
                (stage == "done" ||
                    stage in listOf("releasing", "release_pending", "release_cosigned")) &&
                    round.toInt() == 0 -> {
                    // The arbiter holds a share, not an opinion — and not a
                    // pen. Rulings are a principal's proposal that the
                    // arbiter co-signs; a "proposal" ORIGINATING from the
                    // arbiter's index is nobody asking for their own money
                    // and is refused unheard.
                    if (o.optInt("arbiterIdx") != 0 && senderIdx == o.optInt("arbiterIdx")) {
                        DucatLog.w(TAG, "bond $idHex: proposal from the arbiter refused")
                        return
                    }
                    // Money moving, so the other side's yes is a screen and
                    // not an automatic signature — §15.5's confirm rule
                    // surviving into escrow. Park the proposal;
                    // approveRideRelease() is the tap. A fresh proposal
                    // supersedes a parked, co-signed, or even our own
                    // outstanding one — that last case is the counter-offer:
                    // both sides can propose, and whoever signs ends it.
                    //
                    // **A plain bond used to be exempt** and co-signed here,
                    // on the poller thread, with no screen and no PIN. But
                    // releaseBond sweeps the escrow to the *proposer's* own
                    // wallet, so whichever side tapped "Return the deposit"
                    // first took all of it and the other phone signed that
                    // away by itself on its next poll. The bond's own
                    // description reads "spending it needs both keys, so
                    // neither side can take it alone" — true about keys, false
                    // about consent, which is the only sense a reader means it
                    // in. With an arbiter it was worse: the arbiter is a stock
                    // client too and would rubber-stamp the same sweep.
                    //
                    // The mechanics are unchanged — approveRideRelease does
                    // the identical frostCosign and round-1 send this branch
                    // did. What is added is the person.
                    // What the payload *actually* pays this device, so the
                    // consent screen states the transaction's figure and not
                    // the proposer's. A payload this device cannot read is
                    // refused outright rather than shown: nobody should be
                    // asked to approve bytes we could not open.
                    val toMe = runCatching { releaseToMe(context, o, payload) }
                        .getOrElse { e ->
                            DucatLog.w(
                                TAG,
                                "escrow $idHex: unreadable release proposal — $e",
                            )
                            return@runCatching
                        }
                    o.put("stage", "release_pending")
                    o.put("pendingPayload", payload.toHexString())
                    o.put("proposerIdx", senderIdx)
                    if (riderBackPxmr != null) o.put("pendingRiderBack", riderBackPxmr)
                    else o.remove("pendingRiderBack")
                    if (toMe != null) o.put("pendingToMe", toMe)
                    else o.remove("pendingToMe")
                    save(context, idHex, o)
                    ContactStore.bump()
                    DucatLog.i(TAG, "escrow $idHex: release proposed — waiting for the yes")
                    return@runCatching
                }
                // Not only at "releasing": two counter-offers crossing on
                // the wire put both phones at release_pending, and the
                // co-signature each then sent hit this dispatch's else and
                // was discarded — after which both banners read "Fare
                // released" over money still in the escrow, forever. The
                // engine's proposer session survives those stage moves (it
                // is keyed by ceremony, not stage), so a round 1 answering
                // *our* proposal completes fine from any of the three; one
                // answering nothing falls into the same lost-round recovery
                // below as always.
                stage in listOf("releasing", "release_pending", "release_cosigned") &&
                    round.toInt() == 1 -> {
                    // Only the co-signer we actually asked: in a 2-of-3 an
                    // unsolicited "co-signature" from the third party must
                    // not complete a transaction nobody proposed to them.
                    if (senderIdx != o.optInt("cosignerIdx")) {
                        DucatLog.w(TAG, "bond $idHex: round 1 from unexpected participant $senderIdx")
                        return
                    }
                    val nodeUrl = node(context)
                        ?: throw NoNode()
                    // The claim before the money moves — fundRide's own rule.
                    // frostComplete BROADCASTS; writing "released" only after
                    // it meant a death in the gap left the money gone and the
                    // record saying "releasing", and the completing party is
                    // the payee, whom checkSettled's funder-only gate never
                    // rescued. The marker survives the death and widens that
                    // gate below.
                    o.put("completingAt", System.currentTimeMillis())
                    save(context, idHex, o, durable = true)
                    val txid = uniffi.ducat_mobile.frostComplete(
                        id, i.toUShort(), senderIdx.toUShort(), payload, nodeUrl,
                    )
                    settle(o, "released"); o.put("txid", txid)
                    save(context, idHex, o)
                    // Inventory after the stage is durable, never before: a
                    // death between soldOne and the save re-ran this branch's
                    // recovery and decremented the stock twice for one sale.
                    soldOne(context, o)
                    ContactStore.bump()
                    DucatLog.i(TAG, "bond $idHex released — txid $txid")
                }
                else ->
                    DucatLog.w(TAG, "bond $idHex: frost round $round ignored at stage $stage")
            }
        }.onFailure { why ->
            DucatLog.w(TAG, "bond $idHex frost round $round failed: ${why.message}")
            // The one failure here that strands money, and the one that heals.
            //
            // A release is two rounds and the *proposer* is the side that
            // assembles and broadcasts. The engine's half of that lives in
            // memory, so a proposer whose app is restarted between sending
            // round 0 and the co-signature coming back meets "no release in
            // progress for this ceremony" — and stops. Nothing is broadcast.
            // The co-signer has already written itself "release_cosigned",
            // which reads as finished, so both screens say the deal is done
            // while the money sits in the escrow. Found exactly that way,
            // with a settled banner on one phone and a stuck one on the other.
            //
            // Proposing again is the designed recovery — `onFrostRound`
            // accepts a fresh round 0 over a co-signed one for precisely this
            // reason — so do it rather than wait for somebody to notice and
            // press "Ask again". The split is the one already agreed, read
            // back from the record, so this repeats the proposal rather than
            // inventing a new one.
            // Two spellings of the same stranding. "No release in progress"
            // is the proposer restarted mid-round. InvalidShare is the
            // co-sign racing a supersede: every retry of the proposal
            // derives fresh nonces (the in-memory session cannot survive a
            // restart, so replay is not on offer), and a co-signature built
            // against round N arrives after round N+1 replaced it. Found
            // live 2026-08-28 — the co-signer's screen showed nothing wrong,
            // the proposer's round tore down, and the deal sat until a
            // manual "Ask again". Both heal the same way: repeat the agreed
            // proposal, so the co-signer gets a fresh round 0 to answer.
            val lost = why.message?.contains("no release in progress") == true ||
                why.message?.contains("InvalidShare") == true
            if (lost && round?.toInt() == 1) {
                val back = load(context, idHex)?.optLong("myRiderBack") ?: 0L
                runCatching { proposeRideSplit(context, idHex, back) }
                    .onSuccess { DucatLog.i(TAG, "bond $idHex: release state was lost — proposed again") }
                    .onFailure { DucatLog.w(TAG, "bond $idHex: could not re-propose: ${it.message}") }
            }
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

    /** How long a half-built escrow waits before saying it all again. */
    private const val NUDGE_AFTER_MS = 3L * 60 * 1000

    /**
     * Say it all again, to whoever has not answered.
     *
     * **A lost round used to end the deal.** The mailbox declares a message
     * lost on purpose — a one-time prekey that is already spent has no second
     * reading, and `prekey N is gone; message M is lost` is a designed
     * outcome, not a fault. But the key ceremony had no retransmit: every
     * stage advances only on an inbound round, so one dropped frame left the
     * three parties permanently disagreeing about how far they had got. Seen
     * live 2026-08-25, 2-of-3: the arbiter never received the driver's
     * commitment, sat at `commitment 1/2` for ever, and both phones showed
     * "building the escrow…" with a spinner and no way out until the
     * half-hour sweep deleted the record out from under them.
     *
     * Everything this device has sent goes out again, not just the round it
     * is itself waiting on. The party that is behind is not necessarily the
     * one that is missing something: here the arbiter lacked the driver's
     * *round 0*, while the driver and rider lacked the arbiter's *round 1* —
     * so a device that resent only what it was waiting for would have had
     * every party talking and nobody unblocked.
     *
     * Safe to repeat because it is a retransmit and never a re-derivation.
     * `dkgCommit` draws from OsRng and replaces the engine's machine, and
     * `dkgShare` consumes it; asking either for its bytes a second time would
     * hand two different commitments to two peers, which is a fork, not a
     * retry. So the bytes are kept when they are first sent, and these are
     * those bytes. A receiver records them into a map keyed by the sender's
     * index, so a duplicate overwrites itself and a party who already had
     * them is unaffected.
     */
    fun nudge(context: Context): Int {
        val now = System.currentTimeMillis()
        val due = all(context).filter { o ->
            o.optString("stage") in setOf("committed", "shared") &&
                !isFinished(o) && !isStale(o) &&
                Elapsed.due(now, o.optLong("progressAt", o.optLong("created")), NUDGE_AFTER_MS)
        }
        for (o in due) {
            val idHex = o.optString("id")
            if (idHex.isEmpty()) continue
            runCatching { resend(context, o, null) }
                .onFailure { DucatLog.w(TAG, "escrow $idHex: nudge — ${it.message}") }
            mutate(context, idHex) { cur -> cur.put("progressAt", now) }
        }
        return due.size
    }

    /**
     * Every frame this device has sent for one escrow, to one peer or to all
     * of them. Best-effort per peer: one unreachable party must not stop the
     * others from being caught up.
     */
    private fun resend(context: Context, o: JSONObject, onlyIdx: Int?) {
        val idHex = o.optString("id")
        val id = hexToBytes(idHex) ?: return
        val mineHex = PersonaStore(context).personaHex()
        val roster = o.optJSONArray("roster")?.let { arr ->
            (0 until arr.length()).map { arr.getString(it) }
        } ?: return
        val frame = hexToBytes(o.optString("sent0"))
        val sent1 = o.optJSONObject("sent1")
        var n = 0
        roster.forEachIndexed { zero, peerHex ->
            val idx = zero + 1
            if (peerHex == mineHex) return@forEachIndexed
            if (onlyIdx != null && idx != onlyIdx) return@forEachIndexed
            val peer = contactFor(context, peerHex) ?: return@forEachIndexed
            if (frame != null) {
                runCatching {
                    Mailbox.send(
                        context, peer, "bond: building a shared deposit",
                        kind = 8, round = 0, ceremonyId = id, payload = frame,
                    )
                }.onSuccess { n++ }
                    .onFailure { DucatLog.w(TAG, "escrow $idHex: round 0 again — ${it.message}") }
            }
            sent1?.optString(idx.toString())?.takeIf { it.isNotEmpty() }
                ?.let { hexToBytes(it) }?.let { share ->
                    runCatching {
                        Mailbox.send(
                            context, peer, "bond: your share",
                            kind = 8, round = 1, ceremonyId = id, payload = share,
                        )
                    }.onSuccess { n++ }
                        .onFailure { DucatLog.w(TAG, "escrow $idHex: round 1 again — ${it.message}") }
                }
        }
        if (n > 0) DucatLog.i(TAG, "escrow $idHex: sent $n frame(s) again")
    }

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
    /**
     * Does this record hold the only copy of a threshold key share?
     *
     * Written by dkgFinish once the group key exists. From that moment the
     * escrow has an address on chain, and this share is the device's one way
     * to sign for it — see the note in [sweep].
     */
    fun holdsShare(o: JSONObject): Boolean = o.optString("keys").isNotEmpty()

    fun isStale(o: JSONObject): Boolean {
        if (holdsShare(o)) return false
        if (isFinished(o)) return false
        if (o.optLong("fundedPxmr") > 0) return false
        if (o.optString("fundTxid").isNotEmpty()) return false
        if (o.optString("hostFundTxid").isNotEmpty()) return false
        val created = o.optLong("created").takeIf { it > 0 } ?: return false
        // [Elapsed], because this one decides whether a ceremony ever ends:
        // a `created` stamped ahead of now is never old enough to be stale,
        // so the escrow stays live for ever and the cleanup above never
        // reaches it either.
        return Elapsed.due(System.currentTimeMillis(), created, UNANSWERED_MS)
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

    /**
     * When this party's own money went in, or 0.
     *
     * The clock the banner's stranded branches run on: an escrow only half
     * funded leaves whoever paid first exposed, and §9.3.4's rule — in a
     * system with no operator, "nothing happens" is not a safe default —
     * means that exposure has to name the moment it becomes a way out.
     * Records written before this field existed answer 0, which reads as
     * "long enough", and that is the right side to be wrong on: the money
     * is already in and the offer is only ever an offer.
     */
    /** Whether this party's own money is in the escrow. */
    fun myFundTxidPresent(o: JSONObject): Boolean =
        myFundTxid(o).let { it.isNotEmpty() && it != SENDING }

    fun myFundedAt(o: JSONObject): Long =
        if (isFunder(o)) o.optLong("fundTxidAt") else o.optLong("hostFundTxidAt")

    fun rideWith(context: Context, peerHex: String): JSONObject? =
        dealWith(context, peerHex)?.takeIf { bannerWorthy(it) }

    /**
     * The same escrow [rideWith] would show, without the banner's own filter.
     *
     * The two exist separately because `bannerWorthy` answers *should a person
     * be told about this*, and a day after a ride settles the honest answer is
     * no. That is a display rule, and it read as a state rule exactly once:
     * [Positions.rideIsLive] asked `rideWith` whether the ride was over, got
     * `null` for a ride that had been over since yesterday, and concluded there
     * was no escrow — so the position card came back on a settled thread a day
     * late. Anything asking about the deal's *state* wants this one; only the
     * banner wants the filtered one.
     */
    fun dealWith(context: Context, peerHex: String): JSONObject? =
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

    /** Whether this escrow still has something to say to the person. */
    private fun bannerWorthy(o: JSONObject): Boolean = when (o.optString("stage")) {
        // Settled *or* called off: shown for a day so both sides get the same
        // confirmation, then it is history like any other finished thing.
        //
        // "aborted" used to be `false` — a dead deal says nothing — and the
        // side that did not do the calling off was left with the last live
        // thing it had been told. A rider whose driver cancelled before either
        // stake went in kept "Your ride is coming" on screen, with the whole
        // of the news in a notification, which is the one channel a person
        // swipes away without reading. The reasoning already written for
        // `released` one line down is the reasoning here: the party who did
        // not take the action is the one who has to be told.
        "aborted", "released", "release_cosigned" -> {
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
     * The escrow holds money, so "call it off" is not an ending it has.
     *
     * Typed for the same reason as [AlreadyPaid]: this is reached by
     * circumstance, not by a bug. The banner hides the button once this
     * device's scan has seen the other side's stake, but the scan runs every
     * nine seconds and a stake is paid in one — so the tap that lands in
     * between is ordinary, and used to be answered with the card-link
     * sentence ("the link may be broken, already claimed…") because nothing
     * in `moneyFailure` recognised the refusal.
     */
    class HoldsMoney(val pxmr: Long) : IllegalStateException("there is money in this escrow")

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
     * **And then the chain, once there is an address to ask.** The three
     * markers are what this device has seen, and the scan behind the first
     * runs every nine seconds while the thread is open and never while it is
     * not. A rider who tapped "Call it off" in the gap after the driver's
     * stake landed passed all three, marked their own record aborted, and
     * lost the co-signing path the driver's stake needs to come home: the
     * driver's phone rightly ignores an abort against money it has paid
     * ([onAbort]), so the two records disagreed for good, with the money on
     * the side that could no longer be asked to release it. The Profile
     * screen's bond call-off has asked the address first since it existed;
     * this is the same rule, in the one place both callers pass through.
     * An address that cannot be read is not called empty ([NoNode]) — the
     * cost of refusing is a retry, and the cost of guessing was the stake.
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
        if (o.optLong("fundedPxmr") > 0) throw HoldsMoney(o.optLong("fundedPxmr"))
        check(o.optString("fundTxid").isEmpty()) { "there is money in this escrow" }
        check(o.optString("hostFundTxid").isEmpty()) { "there is money in this escrow" }
        // A release requires funding by construction, whatever this device's
        // scan last said — the same refusal [onAbort] gives the other side.
        check(o.optString("stage") !in listOf("releasing", "release_pending", "release_cosigned")) {
            "a release is in progress"
        }
        hexToBytes(o.optString("keys"))?.let { keys ->
            val nodeUrl = node(context) ?: throw NoNode()
            val from = o.optLong("scanFrom").takeIf { it > 0 }
                ?: WalletStore(context).restoreHeight().toLong()
            val bal = runCatching {
                uniffi.ducat_mobile.escrowBalance(keys, nodeUrl, from.toULong()).toLong()
            }.getOrElse {
                DucatLog.w(TAG, "escrow $idHex: call-off could not read the address: ${it.message}")
                throw NoNode()
            }
            // Not written to `fundedPxmr`: one node's word is enough to
            // refuse an abort and not enough to show money as secured —
            // that figure waits for [checkRideFunding]'s second opinion.
            if (bal > 0) {
                DucatLog.w(TAG, "escrow $idHex: call-off refused — the address holds ${formatXmr(bal)} XMR")
                throw HoldsMoney(bal)
            }
        }

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
                    context, peer, "ceremony: called off",
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
    fun onAbort(context: Context, idHex: String, fromHex: String) {
        val o = load(context, idHex) ?: return
        if (isFinished(o)) return
        // Only somebody in this escrow may end it. onDkgRound and onFrostRound
        // both compute a sender index and bail on a stranger; this one took
        // anybody's word for it.
        val roster = o.optJSONArray("roster")?.let { arr ->
            (0 until arr.length()).map { arr.getString(it) }
        } ?: emptyList()
        if (fromHex !in roster) {
            DucatLog.w(TAG, "escrow $idHex: abort from ${fromHex.take(8)}…, who is not in it")
            return
        }
        // **An arbiter never accepts one.**
        //
        // The three tests below ask whether *this* device has seen the escrow
        // funded, and an arbiter never will: it does not fund, and it is not
        // shown the banner that scans. So it believed every abort it was sent
        // — and "aborted" is not a stage onFrostRound will sign at, so one
        // message permanently disabled the only recovery path a funded 2-of-3
        // has. The victim's "ask the arbiter" button went on working and was
        // silently ignored at the other end.
        //
        // An arbiter is the party whose whole purpose is to still be there
        // when the principals disagree. Whether the money is really gone is
        // not something it can check, and not something it should act on
        // anyone's say-so about: the record simply stays, and the key-share
        // rule in [sweep] keeps it.
        if (isArbiter(o)) {
            DucatLog.i(TAG, "escrow $idHex: an arbiter does not stand down on request")
            return
        }
        if (o.optLong("fundedPxmr") > 0 ||
            o.optString("fundTxid").isNotEmpty() ||
            o.optString("hostFundTxid").isNotEmpty()
        ) {
            DucatLog.w(TAG, "escrow $idHex: abort ignored — it holds money")
            return
        }
        // Those three markers are what THIS device has seen, and on a
        // one-sided ride the non-funding side sees nothing until it happens
        // to open the thread (checkRideFunding's only caller is the
        // banner). A counterparty's abort arriving in that blind window
        // flipped a funded escrow to a terminal "aborted" with every button
        // gone — the driver could never again ask for the fare they had
        // earned. So: a release in progress refuses outright (a release
        // requires funding by construction), and at "done" the escrow's own
        // balance is asked before anybody's word is believed. Only a
        // confirmed-empty address may die on a message.
        val stage = o.optString("stage")
        if (holdsShare(o)) {
            if (stage in listOf("releasing", "release_pending", "release_cosigned")) {
                DucatLog.w(TAG, "escrow $idHex: abort ignored — a release is in progress")
                return
            }
            if (stage == "done") {
                val keys = hexToBytes(o.optString("keys"))
                val nodeUrl = node(context)
                val from = o.optLong("scanFrom").takeIf { it > 0 }
                    ?: WalletStore(context).restoreHeight().toLong()
                val bal = if (keys == null || nodeUrl == null) null else runCatching {
                    uniffi.ducat_mobile.escrowBalance(keys, nodeUrl, from.toULong()).toLong()
                }.getOrNull()
                if (bal == null || bal > 0L) {
                    DucatLog.w(
                        TAG,
                        "escrow $idHex: abort ignored — " +
                            if (bal == null) "could not confirm the address is empty"
                            else "the address holds ${formatXmr(bal)} XMR",
                    )
                    return
                }
            }
        }
        hexToBytes(o.optString("id"))?.let {
            runCatching { uniffi.ducat_mobile.ceremonyAbort(it, o.optInt("i").toUShort()) }
        }
        mutate(context, idHex) { cur -> settle(cur, "aborted") }
        ContactStore.bump()
        DucatLog.i(TAG, "escrow $idHex: the other side called it off")
    }

    /**
     * One fewer of whatever this deal was for.
     *
     * A settled marketplace sale left its listing exactly as it was: "1 left",
     * "Live on the board near you", and buyers still finding it and asking.
     * The app has known which listing since the escrow was built and had no
     * way to act on it — and a count that says one when the one is gone is not
     * a display problem, it is the app stating something untrue.
     *
     * Only the seller does anything here. `aboutListing` is written from the
     * owner's side of an enquiry and is empty on the buyer's, and [Listings.get]
     * answers null for a listing this device does not own, so a buyer's copy of
     * the same escrow falls through both tests.
     *
     * The last one comes down rather than going to zero: a listing is live or
     * it is not, quantity is at least one by construction, and "take it down"
     * is the same verb the seller would have used. It stays saved, so putting
     * it back up is one press if there were more in the cupboard than the app
     * was told about.
     */
    private fun soldOne(context: Context, o: JSONObject) {
        val id = o.optString("aboutListing").ifBlank { return }
        val listing = Listings.get(context, id) ?: return
        val left = Listings.quantityOf(listing)
        if (left > 1) {
            Listings.setQuantity(context, id, left - 1)
            runCatching { Listings.post(context, id) }
                .onFailure { DucatLog.w(TAG, "restock $id: ${it.message}") }
            DucatLog.i(TAG, "sold one of ${listing.optString("title")} — ${left - 1} left")
        } else {
            runCatching { Listings.unpost(context, id) }
                .onFailure { DucatLog.w(TAG, "take down $id: ${it.message}") }
            DucatLog.i(TAG, "sold the last of ${listing.optString("title")} — taken down")
        }
    }

    /**
     * How far a build has actually got, from 0 to 1, or null if it is not one.
     *
     * **Counted, not guessed.** A DKG is two rounds and every frame either
     * side sends is written down as it lands — `commits` and `shares`, keyed by
     * party — so this is arithmetic over what is on disk rather than a bar
     * moving because time is passing. The screen showed a spinner, which says
     * only "something", for a minute of a ceremony that knows exactly how many
     * of its pieces are in.
     *
     * Our own frames count. Reaching `committed` means this device has sent
     * its commitment, and reaching `shared` means it has sent its share; a
     * build that sat at zero until the other side answered would be reporting
     * none of the work it had already done, and looks stopped while it is
     * anything but. So a two-party build has four steps — my commitment,
     * theirs, my share, theirs — and a three-party one has six.
     */
    fun buildProgress(o: JSONObject): kotlin.Float? {
        val stage = o.optString("stage")
        if (stage != "committed" && stage != "shared") return null
        val n = o.optJSONArray("roster")?.length() ?: return null
        if (n < 2) return null
        val commits = o.optJSONObject("commits")?.length() ?: 0
        val shares = o.optJSONObject("shares")?.length() ?: 0
        val done = 1 + commits + if (stage == "shared") 1 + shares else 0
        return (done.toFloat() / (2 * n)).coerceIn(0f, 1f)
    }

    /** Stamp the moment an escrow stopped needing anyone. */
    private fun settle(o: JSONObject, stage: String): JSONObject =
        o.put("stage", stage).put("settledAt", System.currentTimeMillis() / 1000)

    /** The rider pays the fare into the escrow — an ordinary wallet send to
     *  an address that happens to need two of three keys to leave. */
    /** Nobody has disagreed *yet* — the roster has not all reported. */
    class EscrowNotConfirmed : IllegalStateException("waiting for the roster to agree")

    /** Somebody formed a different wallet. This escrow is not what it claims. */
    class EscrowDisagreed : IllegalStateException("participants formed different wallets")

    /**
     * Has every participant reported forming the same wallet as this device?
     *
     * The whole roster, not a majority: a silent participant has not agreed to
     * anything, which is the rule core::escrow::check_escrow_ready states in
     * its first branch and the reason it takes three reports rather than two.
     *
     * Throws rather than returning false on a mismatch, because the two are
     * not the same answer. "Not yet" is a wait; "they formed a different
     * wallet" is a finding, and a caller that treated them alike would sit
     * politely in front of an attack.
     */
    fun escrowAgreed(o: JSONObject): Boolean {
        val roster = o.optJSONArray("roster")?.let { arr ->
            (0 until arr.length()).map { arr.getString(it) }
        } ?: return false
        val mine = o.optString("address")
        if (mine.isEmpty()) return false
        val addrs = o.optJSONObject("addrs") ?: return false
        for (k in addrs.keys()) {
            if (addrs.optString(k) != mine) throw EscrowDisagreed()
        }
        return addrs.length() >= roster.size
    }

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
        // The money moment, and the last place to ask whether everyone built
        // the same wallet. Funding one the roster has not confirmed is the
        // whole cost of a DKG equivocation: the payer's money goes somewhere
        // they hold no share of.
        if (!escrowAgreed(o)) throw EscrowNotConfirmed()
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
                    // A row recovered from a send intent carries no txid —
                    // the hash died with the process — but blank here would
                    // read as "never paid" next time. Any non-empty mark
                    // keeps the guard honest.
                    cur.put(mine, sent.txidHex.ifEmpty { "recovered" })
                    DucatLog.w(TAG, "escrow $idHex: recovered a send nobody recorded")
                    throw AlreadyPaid()
                }
                DucatLog.i(TAG, "escrow $idHex: retaking a claim left by a send that never went")
            } else {
                if (held.isNotEmpty()) throw AlreadyPaid()
            }
            // A live send intent to this address is a payment whose fate the
            // chain has not yet ruled on — the wallet wrote it before
            // broadcasting and only refreshSpent may retire it. Paying again
            // over the top of one is the exact double-fund this whole guard
            // exists to stop; the intent resolves within a refresh either
            // way, so refusing now costs a retry, not the money.
            if (WalletStore(context).sendIntents().any { it.toAddress == addr }) {
                DucatLog.w(TAG, "escrow $idHex: a send intent to this escrow is unresolved — refusing to pay again")
                throw AlreadyPaid()
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
                    .onFailure { e ->
                        // And if *that* write fails, the lockout is real and
                        // permanent: the mark says this party has already
                        // sent, so nothing will ever offer to send again.
                        // At error, because the way back is a human noticing.
                        DucatLog.e(
                            TAG,
                            "escrow $idHex: could not clear the send mark — ${e.message}",
                        )
                    }
            }
            .getOrThrow()
        // Per-party mark: for reservations both sides fund, each records
        // their own send. On a record loaded *now*, not on the snapshot this
        // function started with — rounds arrive on the poller while a
        // transaction is being built, and writing the old object back put the
        // ceremony into a state the protocol had already left.
        // With the moment, not only the txid. Half a funded escrow is a
        // party exposed until the other side follows, and how long they have
        // been exposed is the whole question when deciding whether to offer
        // them a way out (see the banner's stranded branches). Per party,
        // because each is exposed from its own payment.
        mutate(context, idHex) { cur ->
            cur.put(mine, r.txidHex).put("${mine}At", System.currentTimeMillis())
        }
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
        val had = o.optLong("fundedPxmr")
        // Money appearing in an escrow is the claim somebody acts on — the
        // driver drives, the renter matches the host's stake — and it arrived
        // from one node's account of the chain, scanned locally with nothing
        // checking the work behind the blocks. Ask somebody else before
        // showing it as secured. Only growth: a falling balance is a release,
        // and holding that back would strand a record of money already spent.
        if (bal > had && !SecondOpinion.holdsEscrow(context, idHex, keys, from, bal, nodeUrl)) {
            DucatLog.i(TAG, "ride $idHex: escrow growth unconfirmed, holding at ${formatXmr(had)}")
            return had
        }
        if (bal != had) {
            o.put("fundedPxmr", bal)
            save(context, idHex, o)
            ContactStore.bump()
            if (bal > 0) DucatLog.i(TAG, "ride $idHex: escrow holds ${formatXmr(bal)} XMR")
        }
        return bal
    }

    /**
     * Learn from your own wallet that an escrow settled without you.
     *
     * **The 2-of-3 has a third party for exactly the case where a principal
     * is gone, and the party who is gone is the one who never finds out.**
     * escrow_balance says so in its own doc: a multisig output's key image
     * does not exist for any one party, so no single device can read a spend
     * off the escrow address, and what stands in for it is "a device that
     * co-signed knows, because it helped spend it". An arbiter ruling is
     * precisely the release with no co-signature from this side.
     *
     * So the rider's phone came back from being switched off, saw a proposal
     * still parked and a "Sign this split" button, and offered a signature on
     * money that had moved twenty minutes earlier (live, 2026-08-25, txid
     * 3697e203). Tapping it builds a transaction spending inputs that are
     * gone; escrow_balance's own note records what that looks like from the
     * outside — every relay refusing the finished transaction, no reason
     * given, at a fee well above the minimum.
     *
     * The chain can still answer, just from the other end. The funder's
     * residual comes home to a subaddress minted for this ceremony and
     * nothing else, so an output arriving there — in a transaction this
     * device did not send — is the release, found by this device's own scan
     * rather than taken on anybody's word (§17.5).
     *
     * **Only the funder, deliberately.** A driver's payout goes to their
     * ordinary address and an arbiter is owed nothing at all, so neither has
     * an arrival that belongs to one escrow. They are also not the ones
     * holding a button: the proposer knows because it broadcast, and the
     * arbiter never had an opinion to change.
     *
     * One case is left, and left knowingly: a funder owed *nothing* by the
     * release gets no arrival to recognise. That is a deal too small to carry
     * a stake — below [Stakes.FLOOR_PXMR] there is none to hand back — so the
     * banner can still go stale there, on the one shape where the person
     * reading it is owed no money.
     */
    /** Last escrow-balance probe per still-open release, so a stuck record
     *  costs one scan every few minutes rather than one per poller pass. */
    private val settleProbes = java.util.concurrent.ConcurrentHashMap<String, Long>()
    private const val SETTLE_PROBE_GAP_MS = 10L * 60 * 1000

    /** A released record earns doubt only in a window: after the chain has
     *  had ample time to mine it, and not so long after that history gets
     *  re-scanned forever. */
    private const val RELEASED_DOUBT_MIN_SECS = 2L * 3600
    private const val RELEASED_DOUBT_MAX_SECS = 7L * 24 * 3600
    private const val RELEASED_PROBE_GAP_MS = 6L * 3600 * 1000

    fun checkSettled(context: Context): Int {
        val wallet = WalletStore(context)
        val ours by lazy { wallet.ourTxids() }
        val entries by lazy { wallet.entries() }
        var found = 0
        for (o in all(context)) {
            // release_cosigned counts as finished everywhere else — the deal
            // needs nothing more from this device — but it is the one
            // "finished" state whose money this device cannot watch leave,
            // so the settle scan alone keeps an eye on it. Everything else
            // terminal stays skipped.
            if ((isFinished(o) &&
                    o.optString("stage") !in setOf("release_cosigned", "released")) ||
                !holdsShare(o)
            ) continue
            val idHex = o.optString("id")
            if (idHex.isBlank()) continue
            // A completing marker means THIS device called frostComplete and
            // may have died between the broadcast and the write — the one
            // gap the funder-only scan below never rescued, because the
            // completing party is the payee. The escrow's own balance is the
            // truth that survives the death: empty after we tried to spend
            // it means the release happened, whatever the record says.
            if (o.optLong("completingAt") > 0 && o.optString("stage") == "releasing") {
                val keys = hexToBytes(o.optString("keys")) ?: continue
                val nodeUrl = node(context) ?: continue
                val from = o.optLong("scanFrom").takeIf { it > 0 }
                    ?: WalletStore(context).restoreHeight().toLong()
                val bal = runCatching {
                    uniffi.ducat_mobile.escrowBalance(keys, nodeUrl, from.toULong()).toLong()
                }.getOrNull() ?: continue
                if (bal == 0L) {
                    mutate(context, idHex) { cur ->
                        settle(cur, "released").put("stillFundedAt", 0L)
                    }
                    soldOne(context, o)
                    ContactStore.bump()
                    DucatLog.i(TAG, "escrow $idHex: our release landed — recovered after a death mid-broadcast")
                    found += 1
                }
                continue
            }
            // What no arm here CAN do, recorded so nobody rebuilds it: a
            // co-signer at "release_cosigned" has no oracle. escrowBalance
            // measures what ARRIVED — a multisig output's key image exists
            // for no single party, so no scan of ours can see the sweep
            // spend it (ceremony.rs says so in bold), and the other side
            // broadcast, so no txid was ever ours to ask about. A first
            // draft read the balance as "what remains" anyway, decided four
            // finished deals were stranded money, and grew recovery doors
            // into a fiction. The stage is the co-signer's knowledge and it
            // ends at "co-signed"; anything more needs the proposer to send
            // the txid, which is a wire question for another day.
            val stage = o.optString("stage")
            if (stage == "release_cosigned") continue
            // "Released" is a claim about the chain, and the chain can take
            // it back: a broadcast the node accepted can fall out of the
            // mempool unmined. The oracle for that doubt is the recorded
            // txid asked of OTHER nodes (SecondOpinion) — never the escrow
            // balance, for the reason above. SecondOpinion's `false` means
            // "not yet, never never", so demotion also demands real age:
            // hours past settling, a tx no other node has is gone.
            if (stage == "released") {
                val txid = o.optString("txid")
                if (txid.isBlank()) continue
                val settledAt = o.optLong("settledAt")
                val ageSecs = System.currentTimeMillis() / 1000 - settledAt
                if (settledAt <= 0 || ageSecs < RELEASED_DOUBT_MIN_SECS ||
                    ageSecs > RELEASED_DOUBT_MAX_SECS
                ) continue
                val now = System.currentTimeMillis()
                if (now - (settleProbes[idHex] ?: 0L) < RELEASED_PROBE_GAP_MS) continue
                settleProbes[idHex] = now
                if (!SecondOpinion.settles(context, txid)) {
                    mutate(context, idHex) { cur ->
                        cur.put("stage", "releasing")
                        cur.put("wantReleaseAt", System.currentTimeMillis())
                    }
                    ContactStore.bump()
                    DucatLog.w(
                        TAG,
                        "escrow $idHex: release ${txid.take(12)}… unknown to other nodes " +
                            "long after settling — the broadcast never mined; reopening",
                    )
                    found += 1
                }
                continue
            }
            // The healing arm, and the demote's undo: a "releasing" record
            // that carries a txid is one the doubt above reopened (an
            // ordinary proposer records the txid only at completion, with
            // the settle). If other nodes confirm that txid, the release was
            // real all along — settle it back and stop the retries.
            if (stage == "releasing" && o.optString("txid").isNotBlank()) {
                val txid = o.optString("txid")
                val now = System.currentTimeMillis()
                if (now - (settleProbes[idHex] ?: 0L) < SETTLE_PROBE_GAP_MS) continue
                settleProbes[idHex] = now
                if (SecondOpinion.settles(context, txid)) {
                    mutate(context, idHex) { cur ->
                        settle(cur, "released")
                        cur.put("wantReleaseAt", 0L)
                        cur.put("stillFundedAt", 0L)
                    }
                    ContactStore.bump()
                    DucatLog.i(
                        TAG,
                        "escrow $idHex: release ${txid.take(12)}… confirmed after doubt — settled again",
                    )
                    found += 1
                }
                continue
            }

            if (o.optInt("i") != o.optInt("funderIdx")) continue
            val minor = wallet.minorOf("ride_$idHex") ?: continue
            // Not one of ours: the funding transaction and its change are
            // this wallet's own, and the release is the counterparty's.
            val hit = entries.firstOrNull {
                it.minor == minor && it.txHashHex.isNotEmpty() &&
                    it.txHashHex.lowercase() !in ours
            } ?: continue
            // Ending a deal is the same weight as starting one, and one
            // node's account of the chain is a claim (§17.5).
            if (!SecondOpinion.settles(context, hit.txHashHex)) continue
            mutate(context, idHex) { cur -> settle(cur, "released").put("txid", hit.txHashHex) }
            soldOne(context, o)
            ContactStore.bump()
            DucatLog.i(TAG, "escrow $idHex: released without us — txid ${hit.txHashHex}")
            found += 1
        }
        return found
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
        /**
         * Ignore [riderBackPxmr] and ask for exactly what this device put
         * in, leaving everything else where it belongs.
         *
         * For the half-funded escrow, where one side has staked and the
         * other has not come. "Everything back to me" is the wrong claim
         * there and dangerous to make: the balance is read here, at the
         * moment of proposing, and if the other side funded in the seconds
         * between the button and this line, a claim for the whole escrow is
         * a claim for their money too — arriving at an arbiter who has no
         * way to know it was meant innocently. Asking for one's own
         * contribution is the same claim in the case that matters and a
         * harmless one in the case that raced.
         */
        refundMineOnly: Boolean = false,
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
        // This device's own record, which for the funder is an address this
        // device minted — see the note in [join]. Not the invite's.
        val refund = o.optString("refundAddr")
        check(refund.isNotEmpty()) { "this ceremony has no refund address" }
        // **Still open, and worth knowing.** The funder now always has a split
        // it can build that returns its own money. What it cannot yet do is
        // check a split somebody *else* built: the proposer spends to their
        // copy of refundAddr, and the co-signer approves a payload it does not
        // parse, on a screen that states amounts and not destinations. Closing
        // that needs the outputs of the proposed transaction surfaced through
        // the bridge so the approver can be shown where the money goes.
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
        // Against the live balance, never the caller's scan: the whole point
        // of [refundMineOnly] is that a stale figure is what makes the claim
        // wrong. The funder's own money is the rider's slice; everybody
        // else's is the residual, so their own share is what is left after it.
        val mine = mySharePxmr(o)
        val back = when {
            !refundMineOnly -> riderBackPxmr.coerceIn(0L, total)
            isFunder(o) -> mine.coerceIn(0L, total)
            else -> (total - mine).coerceAtLeast(0L)
        }
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
        // **The press outlives the screen.**
        //
        // A release proposed against an escrow the chain has not matured
        // fails, and the banner answers "it will try again on its own — about
        // eighteen minutes left". It did not: the retry was a LaunchedEffect
        // on the chat screen, so it stopped the moment the driver navigated
        // away or pocketed the phone, and eighteen minutes is a long time to
        // stare at a conversation. Found live 2026-08-25, a funded and mature
        // escrow sitting untouched because the driver had gone back to Home.
        //
        // Recorded before the attempt rather than after the failure, because
        // the failure is a throw and this has to survive the process too. It
        // is the *intent*, not a standing instruction: [retryRelease] resends
        // exactly this number, gives up after an hour, and clears the moment
        // a proposal lands (just below).
        mutate(context, idHex) { cur ->
            if (cur.optLong("wantReleaseAt") > 0) cur
            else cur.put("wantRelease", back)
                .put("wantReleaseAt", System.currentTimeMillis())
        }
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
            kind = 9, round = 0, ceremonyId = id, payload = prop.payload,
            amountPxmr = back,
        )
        o.put("stage", "releasing")
        // Asked for, and got there — the intent is spent. Cleared by VALUE,
        // and cleared on the SNAPSHOT so the save below carries the zero:
        // remove() never survived save's merge (fields are added and
        // changed there, never removed — the store's own doc), so every
        // successful proposal left a live-looking intent on the record for
        // good, and the banner went on promising "it will try again"
        // beside deals already settled.
        o.put("wantReleaseAt", 0L)
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

    /** How long "it will try again on its own" is good for. */
    private const val RELEASE_PATIENCE_MS = 60L * 60 * 1000

    /**
     * Finish what the button started, off the screen that started it.
     *
     * The only thing that reaches here is a release somebody asked for and
     * the chain was not ready for — [proposeRideSplit] writes the intent
     * before it can fail, and clears it the moment a proposal lands. So this
     * is not a retry nobody wanted: it is the same press, still trying, for
     * the hour the banner's sentence is worth.
     *
     * Proposing moves no money on its own. It needs the counterparty's
     * signature, which is why proposing was never behind the PIN — and it
     * re-sends the number that was asked for, never a freshly computed
     * default, for the reason "Ask again" learned the hard way.
     */
    fun retryRelease(context: Context): Int {
        val now = System.currentTimeMillis()
        var n = 0
        for (o in all(context)) {
            // Every stage a proposal is legal from — the same list
            // proposeRideSplit accepts. Gating on "done" alone meant a
            // counter-offer that failed at release_pending recorded an
            // intent nobody would ever act on: the press was silently lost.
            if (o.optString("stage") !in
                listOf("done", "releasing", "release_pending", "release_cosigned")
            ) continue
            val idHex = o.optString("id")
            if (idHex.isBlank()) continue
            val asked = o.optLong("wantReleaseAt")
            if (asked <= 0) continue
            if (Elapsed.due(now, asked, RELEASE_PATIENCE_MS)) {
                mutate(context, idHex) { cur -> cur.put("wantReleaseAt", 0L) }
                DucatLog.i(TAG, "escrow $idHex: gave up retrying the release")
                continue
            }
            runCatching { proposeRideSplit(context, idHex, o.optLong("wantRelease")) }
                .onSuccess { n += 1; DucatLog.i(TAG, "escrow $idHex: release proposed on a retry") }
        }
        return n
    }

    /** A release that pays this device less than the screen said it would. */
    class ReleaseMisstated(val statedPxmr: Long, val actualPxmr: Long) :
        IllegalStateException("the proposal pays $actualPxmr, not $statedPxmr")

    /**
     * Every address this deal could legitimately pay this device at.
     *
     * All three are derived from this device's own wallet, never from the
     * wire: the main address a non-funder proposes to itself, the per-contact
     * subaddress it published in the handshake and a counterparty routes the
     * fare to, and the ride-scoped one a funder mints for its own residual.
     * Which of them applies depends on the role and on who proposed, so the
     * set is the answer rather than any one of them.
     */
    private fun myPayoutAddresses(context: Context, o: JSONObject): Set<String> {
        val w = WalletStore(context)
        val out = mutableSetOf<String>()
        w.address()?.let { out += it }
        w.addressFor("ride_${o.optString("id")}")?.let { out += it }
        otherPrincipal(o)?.let { peer -> w.addressFor(peer)?.let { out += it } }
        return out
    }

    /**
     * What a proposed release actually pays this device, read out of the
     * transaction it is being asked to sign — or null when this device cannot
     * tell.
     *
     * §17.5's rule about payments is a rule about consent too. The figure that
     * travels beside a proposal is written by the party who gains from being
     * believed, and until now the co-signer approved a payload it never
     * parsed: the screen stated amounts and the bytes could have paid anyone.
     *
     * Two shapes, because a split has two. A fixed output naming one of this
     * device's addresses is exact, and needs nothing else to check. Being the
     * *residual* claimant instead means taking whatever the fixed outputs and
     * the fee leave behind, so it takes the escrow's total to size — and that
     * total must be this device's own scan ([checkRideFunding], corroborated),
     * never the proposal's word for it. Without one, the answer is null and
     * not zero: unknown and nothing are different answers, and collapsing them
     * would refuse honest releases on a phone that has not scanned yet.
     *
     * The residual figure is stated before the fee, which is the convention
     * the split screen has always used — the fee comes out of the residual
     * side, whoever that is, and is a few ten-thousandths of an XMR.
     */
    fun releaseToMe(context: Context, o: JSONObject, payload: ByteArray): Long? =
        releaseToMe(
            uniffi.ducat_mobile.frostDestinations(payload),
            myPayoutAddresses(context, o),
            o.optLong("fundedPxmr"),
        )

    /** [releaseToMe]'s arithmetic, over outputs already read. */
    fun releaseToMe(
        dests: List<uniffi.ducat_mobile.TxDestination>,
        mine: Set<String>,
        fundedPxmr: Long,
    ): Long? {
        val fixedToMe = dests
            .filter { !it.residual && it.address in mine }
            .sumOf { it.amountPxmr.toLong() }
        if (dests.none { it.residual && it.address in mine }) return fixedToMe
        val fixed = dests.filter { !it.residual }.sumOf { it.amountPxmr.toLong() }
        if (fundedPxmr <= 0 || fundedPxmr < fixed) return null
        return fixedToMe + (fundedPxmr - fixed)
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

        // Read the bytes again at the moment of consent, and refuse to sign
        // anything that pays this device less than the screen stated. The
        // figure was checked once when the proposal arrived; this catches a
        // payload swapped underneath it since, and costs one parse.
        val stated = o.optLong("pendingToMe", -1L)
        if (stated >= 0) {
            val actual = releaseToMe(context, o, payload)
            if (actual != null && actual < stated) {
                DucatLog.w(
                    TAG,
                    "ride $idHex: refusing to sign — the payload pays $actual, not $stated",
                )
                throw ReleaseMisstated(stated, actual)
            }
        }

        val ans = uniffi.ducat_mobile.frostCosign(
            id, i.toUShort(), proposerIdx.toUShort(), keys, payload,
        )
        Mailbox.send(
            context, proposer, "fare released — thank you for the ride",
            kind = 9, round = 1, ceremonyId = id, payload = ans.payload,
        )
        settle(o, "release_cosigned")
        // remove() never survives save's merge; a value-clear does. And the
        // inventory moves only after the stage is durable — a death between
        // soldOne and the save re-ran this path and decremented twice.
        o.put("pendingPayload", "")
        o.put("pendingToMe", -1L)
        save(context, idHex, o)
        soldOne(context, o)
        ContactStore.bump()
        DucatLog.i(TAG, "ride $idHex: rider approved the release (fee ${ans.feePxmr})")
    }
}
