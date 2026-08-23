package org.ducatproject.ducat

import android.content.Context
import uniffi.ducat_mobile.hailDecode
import uniffi.ducat_mobile.standPost
import uniffi.ducat_mobile.standRead

/**
 * Standing at a corner, and looking for somebody standing at one (§15.12).
 *
 * Both halves of a hail used to live inside the screens that drew them — the
 * rider's post inside a button's `onClick`, the driver's read inside a
 * `LaunchedEffect`. That is why the hail was the one flow with no harness at
 * all: the overflow ladder, the deserted-corner copy and the shard climbing
 * are real logic with real edge cases, and none of it could be called by
 * anything that was not a running screen. Renting got its harness precisely
 * because `Listings` was a plain object; this makes the hail the same.
 *
 * Nothing here knows about Compose, and nothing here decides anything a
 * person should decide: the screens still choose when to post, what to
 * charge and what to do about what comes back.
 */
object Hailing {

    /** Every slot on every shard of this cell is taken by a live hail. */
    class BoardFull : IllegalStateException("stand ladder full")

    private const val TAG = "Hail"

    /** How long a hail stands before the boards forget it. */
    const val TTL_SECS = 15L * 60

    /** A hail, as it now stands on the boards. */
    data class Standing(
        val board: String,
        val subkey: UInt,
        val inboxKey: String,
        val cardUri: String,
        val expiry: Long,
        val notice: ByteArray,
        /**
         * True when this corner was deserted — nobody else was standing on
         * the board we landed on. §15.12's density rule: that is exactly when
         * a second copy on the containing 5-cell earns its two round trips.
         */
        val aloneHere: Boolean,
        val originCell: String,
    )

    /** One hail seen on a board, decoded and still live. */
    data class Seen(
        /** Which board it was pinned to — where the clear goes after a claim. */
        val cell: String,
        val subkey: UInt,
        val card: String,
        val dest: String,
        val farePxmr: Long?,
        val expiry: Long,
        val originCell: String?,
        val destCell: String?,
    )

    /**
     * Put a hail on the board for this corner, and remember it.
     *
     * Persists before returning, deliberately: the Home card rehydrates from
     * [RideStore], so a hail must survive the screen that posted it going
     * away mid-write. The 5-cell copy is *not* made here — see [wideCopy] for
     * why it happens after the rider has been told they are standing.
     */
    fun post(
        context: Context,
        originCell: String,
        destCell: String,
        destText: String,
        farePxmr: ULong?,
        ttlSecs: Long = TTL_SECS,
    ): Standing {
        val card = Mailbox.issueCard(
            context, MyProfile(context).name(), (ttlSecs * 2).toULong(), purpose = "hail",
        )
        val expiry = System.currentTimeMillis() / 1000 + ttlSecs
        val info = uniffi.ducat_mobile.HailInfo(
            poster = "",
            card = card.uri,
            dest = destText,
            farePxmr = farePxmr,
            expiry = expiry.toULong(),
            originCell = originCell,
            destCell = destCell,
        )
        val persona = PersonaStore(context).secret()
        // The card's inbox key names this hail: unique to it, the same for the
        // second pin on the containing cell (same author, correctly), and gone
        // when the hail is. The notice is signed for the slot it goes into, so
        // the bytes are built per candidate rather than once — see board.rs.
        fun seal(board: String, slot: UInt): ByteArray =
            uniffi.ducat_mobile.hailEncode(info, persona, card.inboxKey, board, slot)
        var bytes: ByteArray = ByteArray(0)
        // §15.12's overflow ladder, with a read-back: two riders can race for
        // the same free slot and the DHT keeps whoever wrote last, silently.
        // Only a slot that reads back holding our card counts as placed; a
        // lost one just continues the walk.
        val base = "geo:$originCell"
        var placed: Pair<String, UInt>? = null
        // Who else was on the board we landed on, as the ladder saw it.
        var placedTaken: Set<UInt> = emptySet()
        ladder@ for (shard in 0u until uniffi.ducat_mobile.maxStandShards()) {
            val name = uniffi.ducat_mobile.standShardName(base, shard)
            val nowS = System.currentTimeMillis() / 1000
            val taken = standRead(name).mapNotNull { n ->
                runCatching { hailDecode(n.data, name, n.subkey) }.getOrNull()
                    ?.takeIf { it.expiry.toLong() > nowS }
                    ?.let { n.subkey }
            }.toSet()
            for (free in 0u..7u) {
                if (free in taken) continue
                placedTaken = taken
                // standPost verifies its own landing (a refused or raced set
                // throws); re-reading the network here raced its own
                // propagation and read a nearly-empty cell as full.
                val sealed = runCatching { seal(name, free) }.getOrNull() ?: continue
                if (runCatching { standPost(name, free, sealed) }.isSuccess) {
                    bytes = sealed
                    placed = name to free
                    break@ladder
                }
            }
        }
        // Not "every shard is full", which was the sentence a rider actually
        // saw: a ladder is our word for it, and what happened to them is that
        // the corner is busy.
        val (board, sub) = placed ?: throw BoardFull()
        RideStore(context).save(
            RideStore.PostedRide(
                board = board, subkey = sub,
                inboxKey = card.inboxKey, cardUri = card.uri,
                expiry = expiry, notice = bytes,
            ),
        )
        DucatLog.i(TAG, "hail posted at $board subkey $sub")
        return Standing(
            board = board, subkey = sub, inboxKey = card.inboxKey, cardUri = card.uri,
            expiry = expiry, notice = bytes,
            aloneHere = board == base && placedTaken.isEmpty(),
            originCell = originCell,
        )
    }

    /**
     * The deserted-corner copy: a second pin on the containing 5-cell, where
     * a driver kilometres away is actually looking.
     *
     * Called **after** the rider has been told they are standing, not before.
     * The copy is two more network round trips, and a hail is live the moment
     * the first board holds it — waiting for reach that may not even be
     * needed is how posting came to take four minutes behind a spinner.
     *
     * Same card: claim-once referees the two copies the way it referees a
     * migration's.
     */
    fun wideCopy(context: Context, s: Standing): Pair<String, UInt>? {
        if (!s.aloneHere || s.originCell.length != 6) return null
        val wide = "geo:${s.originCell.take(5)}"
        // The same hail, signed again for where it is going. A notice is bound
        // to its slot, so the first board's bytes are not valid on the second
        // — which is the property that stops one signed hail being sprayed
        // across a cell, and it applies to us as much as to anybody.
        //
        // Recovered by opening our own notice rather than threading the fields
        // through Standing: it is the one place that already knows the bytes
        // and the slot they were sealed for.
        val info = runCatching { hailDecode(s.notice, s.board, s.subkey) }.getOrNull()
            ?: return null
        val persona = PersonaStore(context).secret()
        val second = runCatching {
            val busy = standRead(wide).mapNotNull { n ->
                runCatching { hailDecode(n.data, wide, n.subkey) }.getOrNull()
                    ?.takeIf { it.expiry.toLong() > System.currentTimeMillis() / 1000 }
                    ?.let { n.subkey }
            }.toSet()
            (0u..7u).firstOrNull { it !in busy }?.let { s2 ->
                standPost(
                    wide, s2,
                    uniffi.ducat_mobile.hailEncode(info, persona, s.inboxKey, wide, s2),
                )
                wide to s2
            }
        }.getOrNull() ?: return null
        RideStore(context).save(
            RideStore.PostedRide(
                board = s.board, subkey = s.subkey,
                inboxKey = s.inboxKey, cardUri = s.cardUri,
                expiry = s.expiry, notice = s.notice,
                board2 = second.first, subkey2 = second.second,
            ),
        )
        DucatLog.i(TAG, "hail reach: 5-cell copy at ${second.first}")
        return second
    }

    /**
     * One cell's whole ladder, as a driver's sweep reads it.
     *
     * Null when the read itself failed, which a caller must not confuse with
     * an empty corner: a cell whose read failed keeps its last good sweep
     * rather than blinking out of somebody's map.
     *
     * Climb only past a **full** board, the rule `Listings.search` also uses
     * and for the same reason: every writer takes the lowest free slot, so a
     * board with room is the end of its own ladder. The old rule read two
     * shards of every cell and three of any cell that had a hail — and since
     * an empty board takes tens of seconds to come back empty, that chain
     * *was* the lap: 165 s, then 154 s, over eighteen boards.
     */
    fun sweepCell(cell: String, nowSecs: Long): List<Seen>? = runCatching {
        val all = mutableListOf<Seen>()
        var quiet = 0
        for (shard in 0u until uniffi.ducat_mobile.maxStandShards()) {
            val name = uniffi.ducat_mobile.standShardName(cell, shard)
            val live = standRead(name).mapNotNull { n ->
                runCatching { hailDecode(n.data, name, n.subkey) }.getOrNull()?.let { h ->
                    // Expired, or dated so far ahead that it is a squat rather
                    // than a hail — see maxNoticeTtlSecs.
                    val cap = runCatching { uniffi.ducat_mobile.maxNoticeTtlSecs().toLong() }
                        .getOrDefault(31L * 24 * 60 * 60)
                    if (h.expiry.toLong() > nowSecs && h.expiry.toLong() <= nowSecs + cap) {
                        Seen(
                            name, n.subkey, h.card, h.dest,
                            // A board's u64 narrowed to a Long: above 2^63 it
                            // comes out negative, and one of the three fare
                            // entry points takes it without a `> 0` guard. Any
                            // fare that cannot survive the narrowing is not a
                            // fare, so it is dropped here rather than at each
                            // reader — the same rule the two guarded call
                            // sites already apply, applied once.
                            h.farePxmr?.toLong()?.takeIf { it > 0 },
                            h.expiry.toLong(),
                            h.originCell, h.destCell,
                        )
                    } else {
                        null
                    }
                }
            }
            if (live.isNotEmpty()) {
                quiet = 0
                all += live
            } else if (++quiet >= 2) {
                break
            }
            if (live.size < 8) break
        }
        all
    }.getOrNull()
}
