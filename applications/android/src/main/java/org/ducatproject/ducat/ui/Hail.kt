package org.ducatproject.ducat.ui

import org.ducatproject.ducat.securePrefs
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material.icons.filled.MyLocation
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Search
import androidx.compose.ui.draw.clip
import androidx.compose.foundation.horizontalScroll
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.RateStore
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.MainActivity
import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.R
import org.ducatproject.ducat.RideStore
import org.ducatproject.ducat.StoredMessage
import org.ducatproject.ducat.exactXmr
import org.ducatproject.ducat.formatXmr
import uniffi.ducat_mobile.HailInfo
import uniffi.ducat_mobile.hailDecode
import uniffi.ducat_mobile.hailEncode
import uniffi.ducat_mobile.readContactCard
import uniffi.ducat_mobile.standPost
import uniffi.ducat_mobile.standRead

private const val TAG = "Hail"

/** How long a posted hail stands before the board should ignore it. */
private const val HAIL_TTL_SECS = org.ducatproject.ducat.Hailing.TTL_SECS

/** How often the driver re-arms its board watches; they expire. */
private const val WATCH_REARM_MS = 5L * 60 * 1000

/** The longest a quiet network leaves the map unswept. */
private const val SWEEP_BACKSTOP_MS = 45_000L

/**
 * Work the DHT will remember even when the screen does not: posts, claims and
 * slot clears run here, so a navigation or a dismissed sheet cannot cancel
 * them mid-write. One scope for the file, not one per click.
 */
private val hailScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

/** runCatching that lets cancellation through: a swallowed
 *  CancellationException keeps a coroutine limping on after its scope has
 *  told it to stop. */
private inline fun <T> runCatchingCancellable(block: () -> T): Result<T> =
    runCatching(block).onFailure { if (it is CancellationException) throw it }

/**
 * Clear a board slot only if it still holds *our* notice. Slots get reused —
 * a blind empty write races whoever backfilled the slot after us, and the
 * loser is a stranger's live hail. Tenancy is proved by the card URI; an
 * already-empty slot needs nothing, and anything undecodable or belonging to
 * someone else is not ours to erase.
 */
/**
 * Retire every slot a hail occupies — the 6-cell home and any 5-cell copy.
 *
 * Recorded as tombstones *before* the attempt: a take-down that runs while
 * the phone is offline fails silently, and the board kept advertising a
 * withdrawn hail for the next driver to claim (observed live — the desk
 * claimed a ghost). The poller retries what this pass cannot finish.
 */
private fun clearAllSlots(context: android.content.Context, p: PostedHail) {
    val rides = RideStore(context)
    rides.addTombstone(RideStore.Tombstone(p.cell, p.subkey, p.card, p.expiry))
    p.cell2?.let { rides.addTombstone(RideStore.Tombstone(it, p.subkey2, p.card, p.expiry)) }
    sweepHailTombstones(context)
}

/**
 * Try every recorded clear; drop the ones that verifiably took, and the ones
 * whose notice has expired — sweeps filter expired notices and writers treat
 * their slots as free, so past expiry the board heals itself.
 */
fun sweepHailTombstones(context: android.content.Context) {
    val rides = RideStore(context)
    val now = System.currentTimeMillis() / 1000
    rides.tombstones().forEach { t ->
        if (now > t.expiry || clearOwnSlot(t.board, t.subkey, t.card)) {
            rides.removeTombstone(t)
        }
    }
}

/** True when the slot verifiably no longer holds our notice. */
private fun clearOwnSlot(board: String, subkey: UInt, myCardUri: String): Boolean {
    return runCatching {
        val held = standRead(board).firstOrNull { it.subkey == subkey }
            ?: return@runCatching true
        val notice = runCatching { hailDecode(held.data) }.getOrNull()
            ?: return@runCatching true
        if (notice.card != myCardUri) return@runCatching true
        standPost(board, subkey, ByteArray(0))
        // The write can be silently refused (§16.12's read-before-write is
        // primed by the reads above, but the network still referees) — only
        // an empty or foreign read-back counts as cleared.
        standRead(board).firstOrNull { it.subkey == subkey }
            ?.let { runCatching { hailDecode(it.data) }.getOrNull()?.card != myCardUri }
            ?: true
    }.getOrDefault(false)
}

/**
 * Try to rehome a posted notice on a lower shard (§15.12's migration rule).
 *
 * Post-low-first, verify, then clear the old slot: the moment of two copies
 * is safe because both carry the same claim-once card — the DHT referees a
 * race over either copy identically. Returns the new tenancy, or null when
 * nothing lower is free or the landing could not be confirmed.
 */
private fun migrateDown(p: PostedHail): PostedHail? {
    val base = p.cell.substringBeforeLast('-')
    val myShard = p.cell.substringAfterLast('-').toUIntOrNull() ?: return null
    val now = System.currentTimeMillis() / 1000
    return runCatching {
        for (shard in 0u until myShard) {
            val name = uniffi.ducat_mobile.standShardName(base, shard)
            val taken = standRead(name).mapNotNull { n ->
                runCatching { hailDecode(n.data) }.getOrNull()
                    ?.takeIf { it.expiry.toLong() > now }
                    ?.let { n.subkey }
            }.toSet()
            val free = (0u..7u).firstOrNull { it !in taken } ?: continue
            // standPost verifies its own landing; a raced slot throws and the
            // walk simply keeps its current home this round.
            if (runCatching { standPost(name, free, p.notice) }.isSuccess) {
                clearOwnSlot(p.cell, p.subkey, p.card)
                return@runCatching p.copy(cell = name, subkey = free)
            }
        }
        null
    }.getOrNull()
}

/**
 * The rider's half of §15.12, as a card on the personal Home screen.
 *
 * Hailing is a moment, not a job — the modes list is for people running a
 * till or a meter all day, and a rider is neither. So the hail lives where a
 * rider already is: under their balance, folded until wanted. What goes on
 * the board is deliberately nothing: a claim-once card, a coarse destination,
 * an offer, an expiry; the driver who claims it lands in an ordinary
 * conversation, and an unbonded hail is a mutual promise, like flagging a
 * cab — the card says so rather than implying a dispatcher stands behind it.
 */
/**
 * A geocoded endpoint, across an activity recreation.
 *
 * Only the endpoints: the route is recomputed from them by the effect that
 * watches the pair, and its geometry is a polyline that has no business
 * riding in a Binder transaction.
 */
private val HitSaver = androidx.compose.runtime.saveable.listSaver<
    org.ducatproject.ducat.Geo.Hit?, Any>(
    save = { h -> h?.let { listOf(it.label, it.latE7, it.lonE7) } ?: emptyList() },
    restore = { f ->
        if (f.isEmpty()) null
        else org.ducatproject.ducat.Geo.Hit(f[0] as String, f[1] as Long, f[2] as Long)
    },
)

@Composable
fun HailCard(
    /**
     * Opened from the home screen's tile rather than from a card of its own.
     * Hoisted so the trigger and the flow can live apart: the tile is one of
     * three identical squares, and everything below — the sheet, the wait for
     * a driver, the offer, the ride — is this composable's business.
     */
    sheetState: MutableState<Boolean> = remember { mutableStateOf(false) },
) {
    var sheetOpen by sheetState
    var driverFound by remember { mutableStateOf<org.ducatproject.ducat.Contact?>(null) }
    // Between the claim and the yes: the driver who took the hail, while
    // their kind-6 offer is still in flight; then the offer itself, on the
    // decision screen; and the offer set aside when the rider backs out of
    // that screen without deciding — parked, not dropped, so a stray back
    // press cannot silently discard a fare.
    var awaitingOffer by remember { mutableStateOf<org.ducatproject.ducat.Contact?>(null) }
    var rideOffer by remember {
        mutableStateOf<Pair<org.ducatproject.ducat.Contact, StoredMessage>?>(null)
    }
    var parkedOffer by remember {
        mutableStateOf<Pair<org.ducatproject.ducat.Contact, StoredMessage>?>(null)
    }
    val context = LocalContext.current
    var cell by remember { mutableStateOf("") }
    var dest by remember { mutableStateOf("") }
    var fareXmr by remember { mutableStateOf("") }
    val rides = remember { RideStore(context) }
    // The posted hail is DHT state, not screen state: rehydrate from the
    // store so a process restart resumes watching the same slot.
    var posted by remember { mutableStateOf(rides.load()?.asPosted()) }
    var expired by remember { mutableStateOf(false) }
    var status by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }

    // The wait: the card's inbox answers when a driver claims (§16.9's
    // machinery unchanged). Polling, not a watch — a hail lives minutes.
    LaunchedEffect(posted) {
        var p = posted ?: return@LaunchedEffect
        var tick = 0
        while (true) {
            delay(3_000)
            tick += 1
            // §15.12: a notice SHOULD move down when a lower slot frees —
            // the sweep's stopping rule leans on the ladder staying compact,
            // and an overflow shard is a worse address the moment a better
            // one opens. Every tenth tick, not every poll: two board reads.
            if (tick % 10 == 0 && p.cell.contains('-') && p.notice.isNotEmpty()) {
                val moved = withContext(Dispatchers.IO) { migrateDown(p) }
                if (moved != null) {
                    RideStore(context).save(
                        RideStore.PostedRide(
                            board = moved.cell, subkey = moved.subkey,
                            inboxKey = moved.inboxKey, cardUri = moved.card,
                            expiry = moved.expiry, notice = moved.notice,
                        )
                    )
                    DucatLog.i(TAG, "hail moved down to ${moved.cell} slot ${moved.subkey}")
                    // Reassigning `posted` restarts this effect with the new
                    // tenancy — and everything else reading it (the take-down
                    // button above all) must see the slot we actually hold.
                    posted = moved
                    break
                }
            }
            if (System.currentTimeMillis() / 1000 > p.expiry) {
                // Dead either way: retire the notice and say so. The clear
                // rides hailScope so leaving Home cannot cancel it.
                hailScope.launch {
                    clearAllSlots(context, p)
                    rides.clear()
                }
                DucatLog.i(TAG, "hail expired unclaimed")
                status = context.getString(R.string.hail_expired_unclaimed)
                expired = true
                posted = null
                break
            }
            val claimant = withContext(Dispatchers.IO) {
                runCatchingCancellable {
                    Mailbox.collectClaims(context)
                    ContactStore(context).claimantOf(p.inboxKey)
                }.getOrNull()
            }
            if (claimant != null) {
                // Stewardship (§15.12): the notice is spent; clear the slot.
                hailScope.launch {
                    clearAllSlots(context, p)
                    rides.clear()
                }
                val d = ContactStore(context).all().firstOrNull { it.personaHex == claimant }
                DucatLog.i(TAG, "hail claimed by ${d?.displayName() ?: "a driver"}")
                posted = null
                if (d != null) {
                    // The claim is only half the ceremony: the driver's
                    // kind-6 offer names the fare, and the offer screen —
                    // not the chat — is where the rider says yes. Keep
                    // watching the new thread for it.
                    awaitingOffer = d
                } else {
                    // The claim landed but the contact did not materialise;
                    // the chat path still works, so say what is known
                    // rather than nothing.
                    status = context.getString(R.string.hail_claimed_check_chats)
                }
                break
            }
        }
    }

    // The wait for the fare: the offer arrives in the new thread as a kind-6.
    // Polling again rather than watching, and nudging the poller each tick —
    // the background fetch runs on its own clock, and this wait is seconds,
    // not minutes. The Cancel below is the escape hatch, so an offer that
    // never comes cannot strand the rider in silence.
    LaunchedEffect(awaitingOffer) {
        val d = awaitingOffer ?: return@LaunchedEffect
        while (true) {
            delay(3_000)
            val offer = withContext(Dispatchers.IO) {
                runCatchingCancellable {
                    Mailbox.poll(context)
                    // The NEWEST offer, not the first: a repeat rider's thread
                    // already holds last week's kind-6, and accepting a stale
                    // fare against a stale seq leaves the driver waiting on an
                    // answer to an offer they never made (found live,
                    // 2026-08-16 — second hail in one thread).
                    ContactStore(context).thread(d.personaHex)
                        .filter { !it.outgoing && it.kind == 6 }
                        .maxByOrNull { it.timestamp }
                }.getOrNull()
            } ?: continue
            // Re-read the contact: the profile (car, plate) may have landed
            // after the claim did, and the offer screen shows it.
            val fresh = ContactStore(context).all()
                .firstOrNull { it.personaHex == d.personaHex } ?: d
            rideOffer = fresh to offer
            awaitingOffer = null
            status = null
            break
        }
    }

    // Only while something is happening. The way *in* is the tile above; a
    // card that said "Hail a ride — post where you're going" while doing
    // nothing was the entry point, and it is not needed twice.
    if (posted != null || awaitingOffer != null || parkedOffer != null ||
        status != null || error != null
    ) {
    Spacer(Modifier.height(12.dp))
    Card(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
    Column(Modifier.fillMaxWidth().padding(16.dp)) {
        Row(
            Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("🚕", style = MaterialTheme.typography.headlineSmall)
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.hail_card_title), style = MaterialTheme.typography.titleMedium)
                Text(
                    stringResource(R.string.hail_card_standing),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

        }

        val p = posted
        if (p != null) {
            Spacer(Modifier.height(10.dp))
            Text(stringResource(R.string.hail_standing_at,
                    p.cell.removePrefix("geo:").substringBefore("-")),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant)
            // How long it stands. A hail lasts fifteen minutes and then stops
            // being anybody's problem, which is the right design and was
            // invisible: the rider got an indeterminate bar and no way to tell
            // a hail posted thirty seconds ago from one about to lapse. On a
            // kerb that is the only number they want.
            var now by remember { mutableLongStateOf(System.currentTimeMillis() / 1000) }
            LaunchedEffect(p.expiry) {
                while (true) {
                    now = System.currentTimeMillis() / 1000
                    kotlinx.coroutines.delay(5_000)
                }
            }
            val left = (p.expiry - now).coerceAtLeast(0L)
            Spacer(Modifier.height(6.dp))
            Text(
                stringResource(R.string.hail_stands_for, humanDuration(context, left)),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            LinearProgressIndicator(Modifier.fillMaxWidth())
            Spacer(Modifier.height(10.dp))
            OutlinedButton(
                onClick = {
                    val gone = p
                    posted = null; status = null
                    hailScope.launch {
                        clearAllSlots(context, gone)
                        rides.clear()
                    }
                },
                modifier = Modifier.fillMaxWidth(),
            ) { Text(stringResource(R.string.hail_take_it_down)) }
        }

        awaitingOffer?.let { d ->
            Spacer(Modifier.height(10.dp))
            Text(
                stringResource(R.string.hail_awaiting_offer, d.displayName()),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            LinearProgressIndicator(Modifier.fillMaxWidth())
            Spacer(Modifier.height(10.dp))
            OutlinedButton(
                // Sends nothing on purpose: the slot is already cleared and
                // the thread stands — the driver can still say hello there.
                // This only stops the wait.
                onClick = { awaitingOffer = null; status = null },
                modifier = Modifier.fillMaxWidth(),
            ) { Text(stringResource(R.string.hail_cancel)) }
        }
        parkedOffer?.let { (d, o) ->
            Spacer(Modifier.height(10.dp))
            Text(
                stringResource(R.string.hail_parked_offer, d.displayName(),
                    Amounts.show(context, o.amountPxmr).primary),
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(8.dp))
            Button(
                onClick = { rideOffer = parkedOffer; parkedOffer = null },
                modifier = Modifier.fillMaxWidth(),
            ) { Text(stringResource(R.string.hail_view_the_offer)) }
        }
        status?.let {
            if (posted == null) {
                Spacer(Modifier.height(12.dp))
                Text(it, style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.ducat.settled)
                if (expired) {
                    Spacer(Modifier.height(8.dp))
                    OutlinedButton(
                        onClick = { expired = false; status = null; sheetOpen = true },
                        modifier = Modifier.fillMaxWidth(),
                    ) { Text(stringResource(R.string.hail_post_it_again)) }
                }
            }
        }
        error?.let {
            Spacer(Modifier.height(12.dp))
            Text(it, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall)
        }
    }
    }
    }
    driverFound?.let { d ->
        DriverFound(
            contact = d,
            onOpenChat = {
                driverFound = null
                MainActivity.openChat.value = d.personaHex
            },
            onDismiss = { driverFound = null },
        )
    }
    rideOffer?.let { (d, offer) ->
        RideOfferScreen(
            m = offer,
            contact = d,
            onAccept = {
                rideOffer = null
                // The accept echoes the offer's fare and names its seq —
                // that echo is what lets the driver's client hold the two
                // to the same number. Best-effort on hailScope: leaving
                // Home must not lose the yes mid-send.
                hailScope.launch {
                    runCatchingCancellable {
                        Mailbox.send(
                            context, d, context.getString(R.string.hail_accept_message),
                            org.ducatproject.ducat.PersonaStore(context).personaHex(),
                            kind = 7, amountPxmr = offer.amountPxmr,
                            reSeq = offer.seq,
                        )
                    }.onFailure { DucatLog.w(TAG, "ride accept: ${it.message}") }
                    // §15.12: the accept is where the escrow starts, and the
                    // ladder picks the strongest rung available. An arbiter
                    // configured and mutual → 2-of-3 (recovery + judgment).
                    // None → 2-of-2 on mutual stakes: the fare is the
                    // driver's skin, the rider's margin is theirs, and
                    // walking away burns both. The bond banner in the thread
                    // carries it from here either way.
                    val fare = offer.amountPxmr
                    if (fare != null && fare > 0) {
                        val arbHex = org.ducatproject.ducat.ArbiterStore(context).hex()
                            ?.takeIf { it != d.personaHex }
                        val arb = arbHex?.let { h ->
                            org.ducatproject.ducat.ContactStore(context).all()
                                .firstOrNull { it.personaHex == h }
                        }
                        runCatchingCancellable {
                            org.ducatproject.ducat.Ceremony
                                .startRide(
                                    context, d, arb, fare,
                                    // Symmetric by default: the driver
                                    // posts what the rider does, and both
                                    // get it back when the ride ends.
                                    driverStakePxmr =
                                        org.ducatproject.ducat.Ceremony
                                            .rideStakeAmount(fare),
                                )
                        }.onSuccess {
                            DucatLog.i(
                                TAG,
                                "ride escrow started for ${formatXmr(fare)} XMR " +
                                    (if (arb != null) "(2-of-3)" else "(2-of-2 mutual stake)"),
                            )
                        }.onFailure {
                            DucatLog.w(TAG, "ride escrow: ${it.message}")
                        }
                    }
                }
                driverFound = d
            },
            onDecline = {
                rideOffer = null
                hailScope.launch {
                    runCatchingCancellable {
                        Mailbox.send(
                            context, d, context.getString(R.string.hail_decline_message),
                            org.ducatproject.ducat.PersonaStore(context).personaHex(),
                            kind = 5, reSeq = offer.seq, reOwn = false,
                        )
                    }.onFailure { DucatLog.w(TAG, "ride decline: ${it.message}") }
                }
                status = context.getString(R.string.hail_declined_post_again)
                expired = true
            },
            // Backed out without deciding: park it. Re-polling would only
            // re-find the same offer and reopen the screen in the user's
            // face, so the reopen becomes a button instead.
            onClose = { parkedOffer = rideOffer; rideOffer = null },
        )
    }
    if (sheetOpen) {
        HailSheet(
            onPosted = {
                // The sheet persisted the ride before announcing it; the
                // store is the truth even if this card was rebuilt meanwhile.
                posted = rides.load()?.asPosted()
                expired = false
                status = context.getString(R.string.hail_posted_waiting)
            },
            onClose = { sheetOpen = false },
        )
    }
}

private data class PostedHail(
    val cell: String,
    val subkey: UInt,
    val inboxKey: String,
    /** Our card's URI — tenancy proof when the slot is cleared. */
    val card: String,
    /** Epoch seconds; past this the poll gives up and clears. */
    val expiry: Long,
    /** The encoded notice, verbatim — what a migration reposts. */
    val notice: ByteArray = ByteArray(0),
    /** The 5-cell copy, when the corner was deserted (§15.12). */
    val cell2: String? = null,
    val subkey2: UInt = 0u,
)

private fun RideStore.PostedRide.asPosted() =
    PostedHail(board, subkey, inboxKey, cardUri, expiry, notice, board2, subkey2)

/**
 * The driver's offer in flight: persona, our seq, the fare.
 *
 * Just enough to rebuild the waiting banner after process death — the thread
 * itself holds the offer, and the rider's answer names our seq, so nothing
 * else needs remembering. Lives in the same prefs file as the rider's posted
 * hail, under its own key prefix; one offer at a time, like one hail.
 */
private data class DriveOffer(
    val personaHex: String,
    val seq: Long,
    val farePxmr: Long,
    /** Epoch seconds when the offer went out: an answer must be newer. A
     *  re-claimed thread resets sequence numbers, and a years-old accept
     *  whose reSeq collides must never confirm a ride it predates (found
     *  live, 2026-08-16 — a fresh offer "confirmed" by history). */
    val sentAt: Long = 0,
)

private fun ridePrefs(context: android.content.Context) =
    context.getSharedPreferences("ducat_rides", android.content.Context.MODE_PRIVATE)

private fun saveDriveOffer(context: android.content.Context, o: DriveOffer) {
    ridePrefs(context).edit()
        .putString("driveoffer_persona", o.personaHex)
        .putLong("driveoffer_seq", o.seq)
        .putLong("driveoffer_fare", o.farePxmr)
        .putLong("driveoffer_sent", o.sentAt)
        .apply()
}

private fun loadDriveOffer(context: android.content.Context): DriveOffer? {
    val p = ridePrefs(context)
    val persona = p.getString("driveoffer_persona", null) ?: return null
    return DriveOffer(
        persona, p.getLong("driveoffer_seq", 0), p.getLong("driveoffer_fare", 0),
        p.getLong("driveoffer_sent", 0),
    )
}

private fun clearDriveOffer(context: android.content.Context) {
    ridePrefs(context).edit()
        .remove("driveoffer_persona")
        .remove("driveoffer_seq")
        .remove("driveoffer_fare")
        .apply()
}


/**
 * The driver's half: watch a stand, take a hail.
 *
 * The claim is the race, and the DHT referees it — a card has one reply slot,
 * so the second driver's claim simply has nowhere to write and this screen
 * reports "someone beat you to it".
 */
@Composable
fun DriveScreen() {
    val context = LocalContext.current
    var cell by remember { mutableStateOf("") }
    // The watch set: one named stand, or a geocell and its 8 neighbours —
    // §15.12's 3×3, because a rider fifty metres over a border is otherwise
    // invisible.
    // On duty across a trip to the drawer and back.
    //
    // These were screen state, so a driver who opened Settings — or Status, to
    // see whether their node was up — came back off duty, with no hails
    // arriving and nothing saying why. A shift is not a screen. The rider's
    // side already knew this: a posted hail is rehydrated from RideStore
    // precisely so a restart resumes watching the same slot.
    val dutyPrefs = remember { securePrefs(context, "ducat_contacts") }
    var watching by remember {
        mutableStateOf(
            dutyPrefs.getString("drive_watching", null)
                ?.split(",")?.filter { it.isNotBlank() }?.takeIf { it.isNotEmpty() },
        )
    }
    var myFix by remember {
        mutableStateOf(
            dutyPrefs.getLong("drive_lat", Long.MIN_VALUE)
                .takeIf { it != Long.MIN_VALUE }
                ?.let { it to dutyPrefs.getLong("drive_lon", 0L) },
        )
    }
    var notices by remember { mutableStateOf<List<SeenHail>>(emptyList()) }
    // Jobs this driver has taken. Clearing the board is a write the network
    // has to make visible again, and the sweep keeps its last good read of
    // each cell in the meantime — so filtering the taken job out once, at the
    // moment of the claim, lasted exactly until the next lap redrew it from
    // that cached read. A fare you have already taken is not a fare.
    // On disk, like the rest of the shift. Everything else a driver is in the
    // middle of — where they are watching, their box, their range, an offer
    // waiting on a rider — survives a trip to Settings and a process death,
    // because a shift is not a screen. This one list did not, and it is the
    // list that keeps a fare the driver has *already taken* off their map: the
    // board clears by a write somebody has to read back, and until they do,
    // the sweep keeps redrawing the job from its last good read. So a rotation
    // put a claimed fare back on the map, and the driver could offer twice for
    // a rider already in the car.
    var takenCards by remember {
        mutableStateOf(
            dutyPrefs.getString("drive_taken", null)
                ?.split(",")?.filter { it.isNotBlank() }?.toSet() ?: emptySet(),
        )
    }
    var selected by remember { mutableStateOf<SeenHail?>(null) }
    var coverage by remember {
        mutableStateOf(
            dutyPrefs.getString("drive_box", null)
                ?.split(",")?.mapNotNull { it.toLongOrNull() }
                ?.takeIf { it.size == 4 }?.toLongArray(),
        )
    }
    // The net's size is the driver's call: one cell where hails are thick,
    // 5×5 where a fare is worth chasing across a rural county.
    val rangePrefs = remember {
        securePrefs(context, "ducat_contacts")
    }
    var range by remember { mutableStateOf(rangePrefs.getInt("drive_range", 3)) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    // The offer we sent and the rider has not answered. Seeded from prefs so
    // process death mid-wait comes back to the same banner, not to amnesia.
    var pendingOffer by remember { mutableStateOf(loadDriveOffer(context)) }
    var confirmedRide by remember {
        mutableStateOf<Pair<org.ducatproject.ducat.Contact, Long>?>(null)
    }
    var riderDeclined by remember { mutableStateOf(false) }
    val locPerm = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.RequestPermission()
    ) { }
    var locating by remember { mutableStateOf(false) }
    fun driveHere() {
        if (context.checkSelfPermission(android.Manifest.permission.ACCESS_FINE_LOCATION) !=
            android.content.pm.PackageManager.PERMISSION_GRANTED
        ) {
            locPerm.launch(android.Manifest.permission.ACCESS_FINE_LOCATION)
            return
        }
        locating = true
        grabFix(context) { fix ->
            locating = false
            if (fix == null) { error = context.getString(R.string.hail_location_fix_failed); return@grabFix }
            myFix = fix
            runCatching {
                val home = uniffi.ducat_mobile.geohashEncode(fix.first, fix.second, 6u)
                val ring1 = uniffi.ducat_mobile.geohashNeighbors(home)
                // §15.12's density rule: sparse riders drop a copy on their
                // 5-cell, so a driver watches that precision's neighbourhood
                // too — nine more boards, one quiet read each.
                val wide = uniffi.ducat_mobile.geohashEncode(fix.first, fix.second, 5u)
                val wideCells = listOf(wide) + uniffi.ducat_mobile.geohashNeighbors(wide)
                val fine = when (range) {
                    1 -> listOf(home)
                    5 -> (listOf(home) + ring1 +
                        ring1.flatMap { uniffi.ducat_mobile.geohashNeighbors(it) })
                        .distinct()
                    else -> listOf(home) + ring1
                }
                val cells = (fine + wideCells).distinct()
                // The outer box of a contiguous block of cells is the net,
                // drawn on the map so coverage is seen instead of guessed.
                // The box shows the *chosen* range; the 5-cell watch is
                // background reach, not a promise about where you'll drive.
                var latLo = Long.MAX_VALUE; var latHi = Long.MIN_VALUE
                var lonLo = Long.MAX_VALUE; var lonHi = Long.MIN_VALUE
                fine.forEach { c ->
                    val b = uniffi.ducat_mobile.geohashBounds(c)
                    latLo = minOf(latLo, b[0]); latHi = maxOf(latHi, b[1])
                    lonLo = minOf(lonLo, b[2]); lonHi = maxOf(lonHi, b[3])
                }
                coverage = longArrayOf(latLo, latHi, lonLo, lonHi)
                cell = "geo:$home"
                watching = cells.map { "geo:$it" }
                // Remembered, so leaving this screen does not end the shift.
                dutyPrefs.edit()
                    .putString("drive_watching", watching!!.joinToString(","))
                    .putLong("drive_lat", fix.first)
                    .putLong("drive_lon", fix.second)
                    .putString("drive_box", coverage!!.joinToString(","))
                    .apply()
            }.onFailure { error = moneyFailure(context, it) }
        }
    }

    val takeHailShared: (SeenHail, Long) -> Unit = { taken, farePxmr ->
                selected = null
                busy = true; error = null
                hailScope.launch {
                    runCatchingCancellable {
                        val scanned = readContactCard(taken.card)
                        Mailbox.claimCard(context, scanned, null, asDriver = true)
                    }.onSuccess { rider ->
                        // The notice is spent — clear the slot, but only if it
                        // still holds the notice we claimed.
                        clearOwnSlot(taken.cell, taken.subkey, taken.card)
                        // And take it off our own map at once. Clearing the
                        // board is a write someone else has to read back; this
                        // list is what draws the pins here, and leaving the
                        // taken job on it showed the driver a fare still
                        // standing that they had just claimed.
                        withContext(Dispatchers.Main) {
                            takenCards = takenCards + taken.card
                            // Bounded: a card is only worth remembering while
                            // a notice for it could still be redrawn, and a
                            // long shift must not grow this without end.
                            dutyPrefs.edit().putString(
                                "drive_taken",
                                takenCards.toList().takeLast(200).joinToString(","),
                            ).apply()
                            notices = notices.filterNot { it.card == taken.card }
                        }
                        // The offer is protocol now (kind 6): the fare and the
                        // ETA ride typed fields the rider's accept must echo,
                        // and the body stays what a person reads — the car to
                        // scan the curb for. ETA only from a real route; the
                        // old haversine guess was a body-text hedge, not a
                        // number worth a protocol claim.
                        val sent = runCatchingCancellable {
                            val me = org.ducatproject.ducat.MyProfile(context)
                            val fix = myFix
                            val o = taken.originCell?.let {
                                uniffi.ducat_mobile.geohashCenter(it)
                            }
                            val etaSecs = if (fix != null && o != null) {
                                org.ducatproject.ducat.Geo.route(
                                    fix.first, fix.second, o[0], o[1],
                                )?.seconds?.coerceIn(1, 86_400)
                            } else null
                            val car = listOfNotNull(me.carColor(), me.carModel())
                                .joinToString(" ").ifBlank { null }
                            val plate = me.plate()
                            val msg = when {
                                car != null && plate != null -> context.getString(
                                    R.string.hail_on_my_way_car_plate, car, plate)
                                car != null -> context.getString(
                                    R.string.hail_on_my_way_car, car)
                                plate != null -> context.getString(
                                    R.string.hail_on_my_way_plate, plate)
                                else -> context.getString(R.string.hail_on_my_way)
                            }
                            val offerSeq = rider.outSeq
                            Mailbox.send(
                                context, rider, msg,
                                org.ducatproject.ducat.PersonaStore(context).personaHex(),
                                kind = 6, amountPxmr = farePxmr, etaSecs = etaSecs,
                            )
                            DriveOffer(
                                rider.personaHex, offerSeq, farePxmr,
                                System.currentTimeMillis() / 1000,
                            )
                        }.onFailure {
                            DucatLog.w(TAG, "ride offer: ${it.message}")
                        }.getOrNull()
                        DucatLog.i(TAG, "took a hail to ${taken.dest}")
                        if (sent != null) {
                            // The claim survived any process death; the offer
                            // must too, or a relaunched app forgets it is
                            // mid-ceremony.
                            saveDriveOffer(context, sent)
                            withContext(Dispatchers.Main) { pendingOffer = sent }
                        } else {
                            // Claimed but the offer never left: the chat is
                            // the fallback ceremony, as it was before offers
                            // existed.
                            withContext(Dispatchers.Main) {
                                MainActivity.openChat.value = rider.personaHex
                            }
                        }
                    }.onFailure {
                        DucatLog.w(TAG, "claim: ${it.message}")
                        withContext(Dispatchers.Main) {
                            // Being beaten to a fare is only one of the ways
                            // this fails, and it was the answer given for all
                            // of them — including a driver whose node had not
                            // finished connecting, told a rival took the job.
                            error = context.getString(
                                claimFailureRes(it, R.string.hail_beaten_to_it),
                            )
                        }
                    }
                    withContext(Dispatchers.Main) { busy = false }
                }
    }

    // The wait for the rider's word: their kind-7 accept or kind-5 decline
    // names our offer's seq, so the match is exact rather than "any reply".
    // The poller gets a nudge each tick — its own clock is minutes, and a
    // driver already rolling wants seconds.
    LaunchedEffect(pendingOffer) {
        val po = pendingOffer ?: return@LaunchedEffect
        while (true) {
            delay(3_000)
            val answer = withContext(Dispatchers.IO) {
                runCatchingCancellable {
                    Mailbox.poll(context)
                    ContactStore(context).thread(po.personaHex).firstOrNull {
                        !it.outgoing && it.reSeq == po.seq &&
                            it.timestamp >= po.sentAt - 60 &&
                            (it.kind == 7 || (it.kind == 5 && !it.reOwn))
                    }
                }.getOrNull()
            } ?: continue
            clearDriveOffer(context)
            pendingOffer = null
            if (answer.kind == 7) {
                confirmedRide = ContactStore(context).all()
                    .firstOrNull { it.personaHex == po.personaHex }
                    ?.let { it to po.farePxmr }
                DucatLog.i(TAG, "ride confirmed for ${formatXmr(po.farePxmr)} XMR")
            } else {
                riderDeclined = true
                DucatLog.i(TAG, "rider declined the offer")
            }
            break
        }
    }

    confirmedRide?.let { (rider, fare) ->
        RideConfirmed(
            contact = rider,
            farePxmr = fare,
            onOpenChat = {
                confirmedRide = null
                MainActivity.openChat.value = rider.personaHex
            },
            onDismiss = { confirmedRide = null },
        )
    }
    if (riderDeclined) {
        AlertDialog(
            onDismissRequest = { riderDeclined = false },
            title = { Text(stringResource(R.string.hail_rider_declined_title)) },
            text = { Text(stringResource(R.string.hail_rider_declined_body)) },
            confirmButton = {
                TextButton(onClick = { riderDeclined = false }) { Text(stringResource(R.string.hail_ok)) }
            },
        )
    }

    LaunchedEffect(watching) {
        val cells = watching ?: return@LaunchedEffect
        // Every board at once, and each answer drawn the moment it lands.
        //
        // This was three boards per tick, read one after another, four seconds
        // apart. An empty DHT read is tens of seconds — the network concluding
        // a record is not there costs more than finding one — so a driver
        // watching "Nearby" (nine fine cells plus nine wide) waited six ticks
        // for one lap. Measured on two phones: a hail posted at 10:45 reached
        // the driver at 10:52. Nobody stares at an empty map for seven minutes
        // to see whether a fare exists; they put the phone down.
        //
        // The reads do not depend on each other, so they should not queue.
        // Dispatchers.IO carries them concurrently and a lap now costs the
        // slowest single board rather than the sum of eighteen. The rental
        // search has read its ring this way all along (Listings.search) — the
        // hail sweep is the one that never got it.
        val found = java.util.concurrent.ConcurrentHashMap<String, List<SeenHail>>()
        // Ask the network to ring when one of these boards changes, instead of
        // finding out on the next lap. Veilid pushes a change notification to
        // a watcher, and the mailbox has used it all along — a hail board is
        // the same kind of record and nothing was watching it. Re-armed every
        // few minutes because a watch expires: the network promises a wake-up,
        // not delivery, which is exactly why the sweep below stays.
        var armedAt = 0L
        var lastRing = org.ducatproject.ducat.NetworkRings.changed.value
        // Board by record key, filled while arming: a ring says which record
        // moved, and this is what turns that back into a cell.
        val keyOf = HashMap<String, String>()
        // When every board was last read, rings or no rings.
        var sweptAllAt = 0L
        // Hoisted so a cell that answers can be drawn before its neighbours
        // have finished: the first pin is what tells a driver the map works.
        fun publish() {
            val cutoff = System.currentTimeMillis() / 1000
            notices = found.values.flatten()
                .filter { it.expiry > cutoff && it.card !in takenCards }
                // A migrating notice can stand in two slots for a moment;
                // the card is the identity, so one pin, not two.
                .distinctBy { it.card }
                .sortedByDescending { it.expiry }
        }
        while (true) {
            val now = System.currentTimeMillis() / 1000
            if (now * 1000 - armedAt > WATCH_REARM_MS) {
                armedAt = now * 1000
                withContext(Dispatchers.IO) {
                    // standWatch, not nodeDhtWatch: watching needs the record
                    // open in this process and a board is never open — every
                    // reader opens it, reads and closes again. Armed the old
                    // way the network refused it ("record not open"), the
                    // result was discarded, and nothing said so; a driver's
                    // fares were found only by the sweep, a lap late, for as
                    // long as this feature has existed.
                    val armed = cells.count { c ->
                        // The record key alongside, so a ring naming a record
                        // can be turned back into the board it belongs to.
                        // Local arithmetic, no round trip.
                        runCatching { keyOf[c] = uniffi.ducat_mobile.standRecordKey(c) }
                        runCatching { uniffi.ducat_mobile.standWatch(c) }.getOrDefault(false)
                    }
                    // Counted out loud, because the silent version is what
                    // hid this: a number that stays zero is a broken watch.
                    DucatLog.i(TAG, "watching $armed of ${cells.size} board(s)")
                }
            }
            // supervisorScope, and every child swallows its own failure: one
            // unreachable board must not cancel its siblings, and a lap that
            // throws must not end the loop. This is not hypothetical — the
            // sweep logged two laps and then stopped for good, on a phone
            // whose node had not finished attaching, leaving a driver on a map
            // that would never update again and said "No hails standing" while
            // it did nothing.
            // Which boards this lap. A ring names the records that moved, so
            // when they are ours there is no reason to read the other
            // seventeen: the fare is on the one that changed, and a populated
            // board answers in about a second where an empty one takes
            // twenty-one to say it is empty. A ring for something that is not
            // a board of ours — a mailbox, most often — falls back to the full
            // lap, which is what used to happen for every ring.
            //
            // And a full lap on the same clock as the re-arming, whatever the
            // rings say. A watch can die — the network tells us, and that
            // board stops ringing — and in a busy area rings never stop
            // arriving, so a sweep that only ever followed them would read the
            // noisy boards forever and the quiet one never again. The sweep
            // was always the guarantee; this keeps it one.
            val hot = org.ducatproject.ducat.NetworkRings.drain()
            val fullLapDue = now * 1000 - sweptAllAt > WATCH_REARM_MS
            val targets = if (hot.isEmpty() || fullLapDue) {
                sweptAllAt = now * 1000
                cells
            } else {
                cells.filter { keyOf[it] in hot }.ifEmpty {
                    sweptAllAt = now * 1000
                    cells
                }
            }
            kotlinx.coroutines.supervisorScope {
                targets.map { c ->
                    async {
                        runCatchingCancellable {
                // One cell's whole ladder, read the way Hailing defines it —
                // the same function the harness drives, so what a driver sees
                // and what the test proves cannot drift apart.
                val got = withContext(Dispatchers.IO) {
                    org.ducatproject.ducat.Hailing.sweepCell(c, now)
                }
                        // A cell whose read failed keeps its last good sweep
                        // rather than blinking out; publish() filters by
                        // expiry again so a stale notice cannot linger.
                        if (got != null) {
                            found[c] = got
                            withContext(Dispatchers.Main) { publish() }
                        }
                        }.onFailure { DucatLog.w(TAG, "sweep $c: ${it.message}") }
                        Unit
                    }
                }.awaitAll()
            }
            publish()
            // How long a lap actually costs, over how many boards. The whole
            // question for a driver is whether a fare appears in seconds or
            // minutes, and that is not answerable by reading the code.
            DucatLog.i(
                TAG,
                "sweep lap: ${targets.size} board(s)" +
                    (if (targets.size < cells.size) " (the one that rang)" else "") +
                    " in ${System.currentTimeMillis() / 1000 - now}s, " +
                    "${found.values.sumOf { it.size }} notice(s)",
            )
            // Sweep again when the network says something changed, or on a
            // slow backstop if it stays quiet. The ring is what turns "a fare
            // appears within a lap" into "a fare appears while the rider is
            // still putting their phone away": the changed board is populated,
            // and a populated board answers in a fraction of the time an empty
            // one takes to say it is empty.
            //
            // The poller owns the wake-up flag — `node_wait_change` consumes
            // it — so this listens to the poller rather than calling it and
            // stealing somebody's messages.
            withTimeoutOrNull(SWEEP_BACKSTOP_MS) {
                org.ducatproject.ducat.NetworkRings.changed.first { it != lastRing }
            }
            lastRing = org.ducatproject.ducat.NetworkRings.changed.value
        }
    }

    if (watching != null) {
        // On duty: the map is the screen. Demand as pins, me among them,
        // the cards along the bottom for the same jobs in words.
        Column(Modifier.fillMaxSize()) {
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(stringResource(R.string.hail_on_duty), style = MaterialTheme.typography.titleMedium)
                    val n = watching?.size ?: 0
                    val distCtx = androidx.compose.ui.platform.LocalContext.current
                    Text(
                        when {
                            n >= 25 -> stringResource(R.string.hail_net_wide,
                                org.ducatproject.ducat.Units.distance(distCtx, 6000.0))
                            n >= 9 -> stringResource(R.string.hail_net_nearby,
                                org.ducatproject.ducat.Units.distance(distCtx, 3500.0))
                            else -> stringResource(R.string.hail_net_here,
                                org.ducatproject.ducat.Units.distance(distCtx, 1200.0))
                        },
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.outline,
                    )
                }
                OutlinedButton(onClick = { watching = null; notices = emptyList(); forgetDuty(dutyPrefs) }) {
                    Text(stringResource(R.string.hail_stop))
                }
            }
            pendingOffer?.let {
                OfferWaitCard(it, Modifier.fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 4.dp))
            }
            androidx.compose.foundation.layout.Box(Modifier.weight(1f).fillMaxWidth()) {
                DriverMap(
                    coverage = coverage,
                    me = myFix,
                    fares = notices.mapNotNull { n ->
                        n.originCell?.let {
                            runCatching { uniffi.ducat_mobile.geohashCenter(it) }.getOrNull()
                        }?.let { c -> (c[0] to c[1]) to n.dest }
                    },
                    onFareTap = { i -> notices.getOrNull(i)?.let { selected = it } },
                    modifier = Modifier.fillMaxSize(),
                )
                if (notices.isEmpty()) {
                    // "No hails standing" is a claim about the boards, and a
                    // phone whose node has not attached has not read them: the
                    // reads fail in milliseconds and the map states, with
                    // confidence, that nobody wants a ride. Seen on a driver's
                    // phone sixty seconds after launch, while a rider two
                    // metres away was standing on the same cell.
                    val online by produceState(true) {
                        while (true) {
                            value = runCatching {
                                uniffi.ducat_mobile.nodeStatus().publicInternetReady
                            }.getOrDefault(false)
                            delay(4_000)
                        }
                    }
                    Surface(
                        color = MaterialTheme.colorScheme.surface.copy(alpha = 0.85f),
                        shape = MaterialTheme.shapes.medium,
                        modifier = Modifier.align(Alignment.TopCenter).padding(12.dp),
                    ) {
                        Text(
                            stringResource(
                                if (online) R.string.hail_no_hails_map
                                else R.string.hail_joining_network,
                            ),
                            style = MaterialTheme.typography.bodySmall,
                            modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                        )
                    }
                }
                androidx.compose.foundation.lazy.LazyRow(
                    Modifier.align(Alignment.BottomStart).padding(8.dp),
                ) {
                    items(notices.size) { i ->
                        val n = notices[i]
                        Card(
                            onClick = { selected = n },
                            modifier = Modifier.padding(end = 8.dp).width(220.dp),
                        ) {
                            Column(Modifier.padding(10.dp)) {
                                Text(n.dest, style = MaterialTheme.typography.titleSmall,
                                    maxLines = 1)
                                Text(
                                    n.farePxmr?.let {
                                        val sh = Amounts.show(context, it)
                                        sh.secondary ?: sh.primary
                                    } ?: stringResource(R.string.hail_quote_me),
                                    style = MaterialTheme.typography.labelMedium,
                                    color = MaterialTheme.ducat.settled,
                                )
                            }
                        }
                    }
                }
            }
        }
        selected?.let { n ->
            FareDetail(
                n = n, myFix = myFix, busy = busy,
                onTake = takeHailShared, onClose = { selected = null },
            )
        }
        return
    }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp)) {
        Text(stringResource(R.string.hail_driving_title), style = MaterialTheme.typography.headlineSmall)
        Spacer(Modifier.height(4.dp))
        Text(
            stringResource(R.string.hail_driving_pitch),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        pendingOffer?.let {
            Spacer(Modifier.height(12.dp))
            OfferWaitCard(it, Modifier.fillMaxWidth())
        }
        Spacer(Modifier.height(16.dp))

        if (watching == null) {
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                listOf(
                    1 to stringResource(R.string.hail_range_just_here),
                    3 to stringResource(R.string.hail_range_nearby),
                    5 to stringResource(R.string.hail_range_wide),
                ).forEachIndexed { i, (v, label) ->
                    SegmentedButton(
                        selected = range == v,
                        onClick = { range = v; rangePrefs.edit().putInt("drive_range", v).apply() },
                        shape = SegmentedButtonDefaults.itemShape(i, 3),
                        icon = {},
                    ) { Text(label) }
                }
            }
            val rangeCtx = androidx.compose.ui.platform.LocalContext.current
            Text(
                when (range) {
                    1 -> stringResource(R.string.hail_range_desc_here,
                        org.ducatproject.ducat.Units.distance(rangeCtx, 1200.0))
                    5 -> stringResource(R.string.hail_range_desc_wide,
                        org.ducatproject.ducat.Units.distance(rangeCtx, 6000.0))
                    else -> stringResource(R.string.hail_range_desc_nearby,
                        org.ducatproject.ducat.Units.distance(rangeCtx, 3500.0))
                },
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.outline,
            )
            Spacer(Modifier.height(8.dp))
            Button(
                onClick = { driveHere() },
                enabled = !locating,
                modifier = Modifier.fillMaxWidth().height(52.dp),
            ) {
                if (locating) {
                    CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                    Spacer(Modifier.width(8.dp))
                    Text(stringResource(R.string.hail_getting_location))
                } else {
                    Icon(Icons.Filled.MyLocation, null, Modifier.size(18.dp))
                    Spacer(Modifier.width(8.dp))
                    Text(stringResource(R.string.hail_drive_here))
                }
            }
            Spacer(Modifier.height(8.dp))
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(
                value = cell, onValueChange = { cell = it.take(64) },
                label = { Text(stringResource(R.string.hail_stand_label)) },
                placeholder = { Text(stringResource(R.string.hail_stand_placeholder)) },
                modifier = Modifier.weight(1f), singleLine = true,
                enabled = watching == null,
            )
            Spacer(Modifier.width(8.dp))
            if (watching == null) {
                Button(onClick = { watching = listOf(cell.trim()) },
                    enabled = cell.isNotBlank()) { Text(stringResource(R.string.hail_watch)) }
            } else {
                OutlinedButton(onClick = {
                    watching = null; notices = emptyList(); forgetDuty(dutyPrefs)
                }) { Text(stringResource(R.string.hail_stop)) }
            }
        }
        watching?.let { w ->
            if (w.size > 1) {
                Text(
                    stringResource(R.string.hail_watching_neighbours,
                        w.first().removePrefix("geo:")),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.outline,
                )
            }
        }

        if (watching != null) {
            Spacer(Modifier.height(16.dp))
            if (notices.isEmpty()) {
                Text(
                    stringResource(R.string.hail_board_empty),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            notices.forEach { n ->
                Card(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
                    Column(Modifier.padding(16.dp)) {
                        Text(n.dest, style = MaterialTheme.typography.titleMedium)
                        Text(
                            n.farePxmr?.let {
                                val shown = Amounts.show(context, it)
                                shown.secondary?.let { s ->
                                    stringResource(R.string.hail_offers_amount_both,
                                        shown.primary, s)
                                } ?: stringResource(R.string.hail_offers_amount, shown.primary)
                            } ?: stringResource(R.string.hail_asks_quote),
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        // The triage line (§16.17): how far to the fare, how
                        // long the job — from the cells, all the board knows.
                        val fix = myFix
                        val triage = remember(n, fix) {
                            runCatching {
                                val parts = ArrayList<String>()
                                val o = n.originCell?.let {
                                    uniffi.ducat_mobile.geohashCenter(it)
                                }
                                if (o != null && fix != null) {
                                    val m = uniffi.ducat_mobile.haversineM(
                                        fix.first, fix.second, o[0], o[1])
                                    parts += context.getString(
                                        R.string.hail_triage_pickup,
                                        org.ducatproject.ducat.Units.distance(
                                            context, m.toDouble()))
                                }
                                val d = n.destCell?.let {
                                    uniffi.ducat_mobile.geohashCenter(it)
                                }
                                if (o != null && d != null) {
                                    val m = uniffi.ducat_mobile.haversineM(
                                        o[0], o[1], d[0], d[1])
                                    parts += context.getString(
                                        R.string.hail_triage_trip,
                                        org.ducatproject.ducat.Units.distance(context,
                                            m.toDouble() * org.ducatproject.ducat.Fare.CIRCUITY))
                                }
                                parts.joinToString(" · ")
                            }.getOrDefault("")
                        }
                        if (triage.isNotEmpty()) {
                            Text(triage, style = MaterialTheme.typography.labelMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        // A notice can expire between sweeps; a negative
                        // countdown is nonsense, so say nothing instead.
                        val mins = (n.expiry - System.currentTimeMillis() / 1000) / 60
                        if (mins > 0) {
                            Text(stringResource(R.string.hail_stands_for_min, mins),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.outline)
                        }
                        Spacer(Modifier.height(10.dp))
                        Row {
                        OutlinedButton(
                            onClick = { selected = n },
                            modifier = Modifier.weight(1f).height(44.dp),
                        ) { Text(stringResource(R.string.hail_view_job)) }
                        Spacer(Modifier.width(8.dp))
                        Button(
                            onClick = {
                                // A named fare can be taken as offered in one
                                // tap; "quote me" needs a number typed, which
                                // is the detail screen's job.
                                val fare = n.farePxmr
                                if (fare != null) takeHailShared(n, fare)
                                else selected = n
                            },
                            enabled = !busy,
                            modifier = Modifier.weight(1f).height(44.dp),
                        ) { Text(stringResource(R.string.hail_take_it)) }
                        }
                    }
                }
            }
        }

        error?.let {
            Spacer(Modifier.height(10.dp))
            Text(it, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall)
        }
    }

    selected?.let { n ->
        FareDetail(
            n = n,
            myFix = myFix,
            busy = busy,
            onTake = takeHailShared,
            onClose = { selected = null },
        )
    }
}

/**
 * Parse an offer/fare field to picoXMR, converting from the local currency when
 * the field is denominated in it. Null if the text does not parse, or if a rate
 * is needed but not cached. Mirrors Taxi/Pos so every money entry in the app
 * behaves identically.
 */
internal fun offerToPxmr(text: String, fiat: Boolean, rate: Double?): Long? {
    val v = moneyText(text).toBigDecimalOrNull() ?: return null
    val xmr = if (fiat) {
        if (rate == null || rate <= 0) return null
        v.divide(java.math.BigDecimal.valueOf(rate), 12, java.math.RoundingMode.DOWN)
    } else v
    return Amounts.toPxmr(xmr)?.takeIf { it >= 0 }
}

/** Render picoXMR into a field string in the chosen unit (the inverse used to
 *  seed and to convert a field when the unit toggle is flipped). */
internal fun pxmrToField(pxmr: Long, fiat: Boolean, rate: Double?): String =
    if (fiat && rate != null && rate > 0) {
        "%.2f".format(java.util.Locale.US, pxmr / 1e12 * rate)
    } else {
        // Not formatXmr. Whatever goes in here comes back out through
        // `moneyText(…).toBigDecimalOrNull()` when the offer is read, and
        // formatXmr is written for eyes: it localizes its digits, so flipping
        // the unit toggle on a Persian phone seeded the field with ۰.۰۳۸۰۰۰
        // and the offer could no longer be read at all — and for a small
        // enough amount it answers the literal "<0.000001", which parses to
        // nothing anywhere. Exact, ASCII, and no trailing noise, so flipping
        // the toggle back and forth does not drift the number.
        exactXmr(pxmr).trimEnd('0').trimEnd('.')
    }

/**
 * The job, before the yes: the whole trip on one map — me to the pickup, the
 * ride itself — with what it pays against what a platform would have paid.
 * The Uber driver's accept screen, minus the countdown pressure: a hail
 * stands until it expires, and a decision under a shrinking timer is not a
 * decision.
 */
@Composable
private fun FareDetail(
    n: SeenHail,
    myFix: Pair<Long, Long>?,
    busy: Boolean,
    onTake: (SeenHail, Long) -> Unit,
    onClose: () -> Unit,
) {
    val context = LocalContext.current
    var route by remember { mutableStateOf<org.ducatproject.ducat.Geo.Route?>(null) }
    // The fare field can be typed in the local currency, like every other money
    // entry (Pay/POS/Taxi). It starts in whatever unit the wallet prefers, but
    // only if a rate is cached — without one, fiat entry has nothing to convert.
    val rate = remember { RateStore(context).cached()?.first }
    val cur = remember { Amounts.currency(context) }
    val initFiat = remember { Amounts.preferFiat(context) && rate != null }
    var fiat by remember { mutableStateOf(initFiat) }
    // The fare the Take will put on the wire (kind 6): the rider's offer to
    // start with, editable because a counter-offer is a fare too. A "quote
    // me" hail seeds from the routed estimate once one exists.
    var fareXmr by remember(n) {
        mutableStateOf(n.farePxmr?.let { pxmrToField(it, initFiat, rate) } ?: "")
    }
    var fareError by remember(n) { mutableStateOf<String?>(null) }
    val origin = remember(n) {
        n.originCell?.let {
            runCatching { uniffi.ducat_mobile.geohashCenter(it) }.getOrNull()
        }
    }
    val dest = remember(n) {
        n.destCell?.let {
            runCatching { uniffi.ducat_mobile.geohashCenter(it) }.getOrNull()
        }
    }
    LaunchedEffect(n, myFix) {
        val o = origin ?: return@LaunchedEffect
        val pts = ArrayList<Pair<Long, Long>>()
        myFix?.let { pts += it }
        pts += (o[0] to o[1])
        dest?.let { pts += (it[0] to it[1]) }
        if (pts.size >= 2) {
            route = withContext(Dispatchers.IO) {
                org.ducatproject.ducat.Geo.routeVia(pts)
            }
            if (n.farePxmr == null && fareXmr.isBlank()) {
                route?.let { r ->
                    val trip = if (myFix != null && r.legs.size >= 2) r.legs[1]
                    else r.legs.firstOrNull()
                    trip?.let { (mtr, secs) ->
                        org.ducatproject.ducat.Fare.estimateExact(context, mtr, secs)
                            ?.let { (_, pxmr) -> fareXmr = pxmrToField(pxmr, fiat, rate) }
                    }
                }
            }
        }
    }

    androidx.compose.ui.window.Dialog(
        onDismissRequest = onClose,
        properties = androidx.compose.ui.window.DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            Column(Modifier.fillMaxSize().verticalScroll(
                androidx.compose.foundation.rememberScrollState())) {
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    IconButton(onClick = onClose) { Icon(Icons.Filled.Close, stringResource(R.string.hail_close)) }
                    Text(stringResource(R.string.hail_job_title), style = MaterialTheme.typography.titleLarge)
                }
                RouteMap(
                    from = myFix ?: origin?.let { it[0] to it[1] },
                    to = dest?.let { it[0] to it[1] } ?: origin?.let { it[0] to it[1] },
                    route = route?.points ?: emptyList(),
                    modifier = Modifier.fillMaxWidth().height(300.dp)
                        .padding(horizontal = 16.dp)
                        .clip(MaterialTheme.shapes.large),
                )
                Column(Modifier.padding(16.dp)) {
                    Text(n.dest, style = MaterialTheme.typography.titleMedium)
                    route?.let { r ->
                        val toPickup = r.legs.getOrNull(0)
                        val trip = if (r.legs.size >= 2) r.legs[1] else r.legs.getOrNull(0)
                        toPickup?.let {
                            if (myFix != null) Text(
                                stringResource(R.string.hail_to_pickup,
                                    org.ducatproject.ducat.Units.distance(
                                        context, it.first.toDouble()),
                                    (it.second / 60).coerceAtLeast(1)),
                                style = MaterialTheme.typography.bodyMedium,
                            )
                        }
                        trip?.let {
                            Text(
                                stringResource(R.string.hail_the_ride,
                                    org.ducatproject.ducat.Units.distance(
                                        context, it.first.toDouble()),
                                    (it.second / 60).coerceAtLeast(1)),
                                style = MaterialTheme.typography.bodyMedium,
                            )
                        }
                        Spacer(Modifier.height(8.dp))
                        n.farePxmr?.let { fare ->
                            val shown = Amounts.show(context, fare)
                            Text(
                                shown.secondary?.let { s ->
                                    stringResource(R.string.hail_offer_amount_both,
                                        shown.primary, s)
                                } ?: stringResource(R.string.hail_offer_amount, shown.primary),
                                style = MaterialTheme.typography.titleMedium,
                            )
                            trip?.let { t ->
                                val (_, uberDriverUsd, _) = org.ducatproject.ducat.Fare
                                    .competitors(context, t.first, t.second)
                                val cur = Amounts.currency(context)
                                // In the reader's money, not the table's: this
                                // printed a dollar figure beside their own
                                // currency code.
                                val uberDriver =
                                    org.ducatproject.ducat.Fare.usdToReader(context, uberDriverUsd)
                                Text(
                                    stringResource(R.string.hail_keep_all_vs_rideshare,
                                        cur, uberDriver ?: 0.0),
                                    style = MaterialTheme.typography.labelMedium,
                                    color = MaterialTheme.ducat.settled,
                                )
                            }
                        } ?: Text(stringResource(R.string.hail_rider_asks_quote),
                            style = MaterialTheme.typography.titleMedium)
                    } ?: Row(verticalAlignment = Alignment.CenterVertically) {
                        CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                        Spacer(Modifier.width(8.dp))
                        Text(stringResource(R.string.hail_routing_job), style = MaterialTheme.typography.bodySmall)
                    }
                    Spacer(Modifier.height(14.dp))
                    OutlinedTextField(
                        value = fareXmr,
                        onValueChange = { fareXmr = it; fareError = null },
                        label = { Text(stringResource(R.string.hail_your_fare, if (fiat) cur else "XMR")) },
                        supportingText = {
                            Text(
                                if (n.farePxmr != null) stringResource(R.string.hail_offer_counter_hint)
                                else stringResource(R.string.hail_offer_quote_hint),
                            )
                        },
                        modifier = Modifier.fillMaxWidth(), singleLine = true,
                    )
                    if (rate != null) {
                        TextButton(onClick = {
                            // Convert what is typed so flipping the unit keeps
                            // the same money, not the same digits.
                            val pxmr = offerToPxmr(fareXmr, fiat, rate)
                            fiat = !fiat
                            fareXmr = pxmr?.let { pxmrToField(it, fiat, rate) } ?: ""
                            fareError = null
                        }) { Text(stringResource(R.string.hail_price_in_instead, if (fiat) "XMR" else cur)) }
                    }
                    Spacer(Modifier.height(10.dp))
                    Button(
                        onClick = onClick@{
                            // Same refusal as the rider's sheet: a comma
                            // keyboard, a minus, or garbage must fail loudly —
                            // an offer for a fare nobody typed is a silent
                            // repricing.
                            val pxmr = offerToPxmr(fareXmr, fiat, rate)
                            if (pxmr == null || pxmr <= 0) {
                                fareError = context.getString(R.string.hail_fare_invalid)
                                return@onClick
                            }
                            onTake(n, pxmr)
                        },
                        enabled = !busy,
                        modifier = Modifier.fillMaxWidth().height(52.dp),
                    ) { Text(stringResource(R.string.hail_take_this_ride)) }
                    fareError?.let {
                        Spacer(Modifier.height(4.dp))
                        Text(it, color = MaterialTheme.colorScheme.error,
                            style = MaterialTheme.typography.bodySmall)
                    }
                    Spacer(Modifier.height(6.dp))
                    OutlinedButton(
                        onClick = onClose,
                        modifier = Modifier.fillMaxWidth().height(44.dp),
                    ) { Text(stringResource(R.string.hail_pass)) }
                }
            }
        }
    }
}

/**
 * Claimed but unanswered: the compact card that keeps the wait visible
 * without taking the map away — a driver can keep cruising while the rider
 * decides, and a full-screen wait would say otherwise.
 */
@Composable
private fun OfferWaitCard(po: DriveOffer, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val name = remember(po.personaHex) {
        ContactStore(context).all()
            .firstOrNull { it.personaHex == po.personaHex }?.displayName()
    }
    Card(modifier) {
        Column(Modifier.padding(12.dp)) {
            Text(
                stringResource(R.string.hail_offered_waiting,
                    Amounts.show(context, po.farePxmr).primary,
                    name ?: stringResource(R.string.hail_the_rider)),
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(6.dp))
            LinearProgressIndicator(Modifier.fillMaxWidth())
        }
    }
}

// The hail's protocol half lives in Hailing.kt now, so that something other
// than a running screen can drive it — see the harness. This keeps the name
// the screen has always used.
private typealias SeenHail = org.ducatproject.ducat.Hailing.Seen

/**
 * The full hail: addresses, a map, a routed fare — geocells never shown.
 *
 * The geohash machinery (§15.12) runs entirely backstage: this screen speaks
 * in addresses and pins, and the cells are computed at post time. One privacy
 * line is owed and said: address search, routing and map tiles all query
 * OpenStreetMap's servers, which is a location disclosure nothing else in
 * DUCAT makes. The board itself still carries only cells and short text.
 */
@Composable
fun HailSheet(
    onPosted: () -> Unit, // the posted ride is read back from RideStore
    onClose: () -> Unit,
) {
    val context = LocalContext.current
    // Both ends survive a recreation. Each one cost a typed search and a
    // geocoder round-trip, and a rotation or a sunset theme switch used to
    // throw away the pair and the route with them.
    var from by rememberSaveable(stateSaver = HitSaver) {
        mutableStateOf<org.ducatproject.ducat.Geo.Hit?>(null)
    }
    var to by rememberSaveable(stateSaver = HitSaver) {
        mutableStateOf<org.ducatproject.ducat.Geo.Hit?>(null)
    }
    var route by remember { mutableStateOf<org.ducatproject.ducat.Geo.Route?>(null) }
    var routing by remember { mutableStateOf(false) }
    // The offer can be typed in the local currency, like the rest of the app.
    // Only defaults to fiat when a rate is cached to convert it.
    val rate = remember { RateStore(context).cached()?.first }
    val cur = remember { Amounts.currency(context) }
    var fiat by rememberSaveable { mutableStateOf(Amounts.preferFiat(context) && rate != null) }
    var fareXmr by rememberSaveable { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    val locPerm = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.RequestPermission()
    ) { ok ->
        if (ok) grabFix(context) { f ->
            from = f?.let {
                org.ducatproject.ducat.Geo.Hit(
                    context.getString(R.string.hail_my_location), it.first, it.second)
            }
        }
    }
    var locating by remember { mutableStateOf(false) }
    fun useMyLocation() {
        if (context.checkSelfPermission(android.Manifest.permission.ACCESS_FINE_LOCATION) ==
            android.content.pm.PackageManager.PERMISSION_GRANTED
        ) {
            locating = true
            grabFix(context) { f ->
                locating = false
                from = f?.let {
                    org.ducatproject.ducat.Geo.Hit(
                        context.getString(R.string.hail_my_location), it.first, it.second)
                }
                if (f == null) error = context.getString(R.string.hail_location_fix_failed)
            }
        } else locPerm.launch(android.Manifest.permission.ACCESS_FINE_LOCATION)
    }
    // Only when there is nothing to restore: after a recreation this would
    // otherwise overwrite the pickup somebody had chosen with wherever the
    // phone happens to be.
    LaunchedEffect(Unit) { if (from == null) useMyLocation() }

    // Addresses first; the route (and the map that draws it) only once both
    // ends exist. The map is a preview of a decision, not a picker.
    LaunchedEffect(from, to) {
        // Editing an endpoint retracts the route: a stale route under a
        // cleared field left a Hail button pointing at !!-nothing.
        val f = from ?: run { route = null; return@LaunchedEffect }
        val t = to ?: run { route = null; return@LaunchedEffect }
        routing = true
        route = null
        val r = withContext(Dispatchers.IO) {
            org.ducatproject.ducat.Geo.route(f.latE7, f.lonE7, t.latE7, t.lonE7)
        }
        routing = false
        route = r
        if (r == null) {
            error = context.getString(R.string.hail_no_route)
        } else {
            error = null
            org.ducatproject.ducat.Fare.estimateExact(context, r.meters, r.seconds)
                ?.let { (_, pxmr) -> fareXmr = pxmrToField(pxmr, fiat, rate) }
        }
    }

    androidx.compose.ui.window.Dialog(
        onDismissRequest = onClose,
        properties = androidx.compose.ui.window.DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            Column(
                Modifier.fillMaxSize()
                    .verticalScroll(androidx.compose.foundation.rememberScrollState())
                    .imePadding(),
            ) {
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    IconButton(onClick = onClose) { Icon(Icons.Filled.Close, stringResource(R.string.hail_close)) }
                    Text(stringResource(R.string.hail_card_title), style = MaterialTheme.typography.titleLarge)
                }

                Column(Modifier.padding(horizontal = 16.dp)) {
                    AddressField(
                        label = stringResource(R.string.hail_pickup_label),
                        chosen = from,
                        onChosen = { from = it },
                        near = from?.let { it.latE7 to it.lonE7 },
                        hint = if (locating) stringResource(R.string.hail_locating) else null,
                        trailing = {
                            FilledTonalIconButton(onClick = { useMyLocation() }) {
                                Icon(Icons.Filled.MyLocation, stringResource(R.string.hail_use_my_location))
                            }
                        },
                    )
                    Spacer(Modifier.height(8.dp))
                    AddressField(
                        label = stringResource(R.string.hail_where_to),
                        chosen = to,
                        onChosen = { to = it },
                        // Bias toward the pickup: a business name means the
                        // one near you, not the one in another hemisphere.
                        near = from?.let { it.latE7 to it.lonE7 },
                        hint = stringResource(R.string.hail_address_hint),
                    )

                    if (routing) {
                        Spacer(Modifier.height(16.dp))
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                            Spacer(Modifier.width(8.dp))
                            Text(stringResource(R.string.hail_finding_route),
                                style = MaterialTheme.typography.bodySmall)
                        }
                    }

                    route?.let { r ->
                        Spacer(Modifier.height(12.dp))
                        RouteMap(
                            from = from?.let { it.latE7 to it.lonE7 },
                            to = to?.let { it.latE7 to it.lonE7 },
                            route = r.points,
                            modifier = Modifier.fillMaxWidth().height(260.dp)
                                .clip(MaterialTheme.shapes.large),
                        )
                        Spacer(Modifier.height(10.dp))
                        val est = org.ducatproject.ducat.Fare
                            .estimateExact(context, r.meters, r.seconds)
                        val cur = remember { Amounts.currency(context) }
                        // One number, both units. The route is known; a range
                        // was the estimate hedging against a distance we had
                        // already measured.
                        Text(
                            est?.let {
                                stringResource(R.string.hail_route_summary_priced,
                                    org.ducatproject.ducat.Units.distance(
                                        context, r.meters.toDouble()),
                                    r.seconds / 60, cur, it.first)
                            } ?: stringResource(R.string.hail_route_summary,
                                org.ducatproject.ducat.Units.distance(
                                    context, r.meters.toDouble()),
                                r.seconds / 60),
                            style = MaterialTheme.typography.titleMedium,
                        )
                        est?.let { (fiat, _) ->
                            val (uberUsd, driverUsd, taxiUsd) =
                                org.ducatproject.ducat.Fare.competitors(context, r.meters, r.seconds)
                            fun here(d: Double) =
                                org.ducatproject.ducat.Fare.usdToReader(context, d) ?: 0.0
                            Text(
                                stringResource(R.string.hail_vs_competitors,
                                    cur, here(uberUsd), here(taxiUsd), here(driverUsd)),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.ducat.settled,
                            )
                        }
                        Spacer(Modifier.height(8.dp))
                        OutlinedTextField(
                            value = fareXmr,
                            onValueChange = { fareXmr = it },
                            label = { Text(stringResource(R.string.hail_your_offer, if (fiat) cur else "XMR")) },
                            modifier = Modifier.fillMaxWidth(), singleLine = true,
                        )
                        if (rate != null) {
                            TextButton(onClick = {
                                // Convert what is typed so the unit flip keeps
                                // the same money, not the same digits.
                                val pxmr = offerToPxmr(fareXmr, fiat, rate)
                                fiat = !fiat
                                fareXmr = pxmr?.let { pxmrToField(it, fiat, rate) } ?: ""
                            }) { Text(stringResource(R.string.hail_price_in_instead, if (fiat) "XMR" else cur)) }
                        }
                        Spacer(Modifier.height(10.dp))
                        Button(
                            onClick = onClick@{
                                // A comma-decimal keyboard types "0,004", and
                                // a minus or garbage must refuse loudly here —
                                // posting "quote me" instead of the typed
                                // offer is a silent repricing.
                                var fare: ULong? = null
                                if (fareXmr.trim().isNotEmpty()) {
                                    val pxmr = offerToPxmr(fareXmr, fiat, rate)
                                    if (pxmr == null || pxmr <= 0) {
                                        error = context.getString(R.string.hail_offer_invalid)
                                        return@onClick
                                    }
                                    fare = pxmr.toULong()
                                }
                                busy = true; error = null
                                val f = from!!
                                val t = to!!
                                val destText = t.label.take(64)
                                // The post outlives the sheet on purpose: a
                                // dismissal mid-write must not orphan a live
                                // notice on the board.
                                hailScope.launch {
                                    runCatchingCancellable {
                                        val oCell = uniffi.ducat_mobile.geohashEncode(f.latE7, f.lonE7, 6u)
                                        val dCell = uniffi.ducat_mobile.geohashEncode(t.latE7, t.lonE7, 6u)
                                        val standing = org.ducatproject.ducat.Hailing.post(
                                            context, oCell, dCell, destText, fare,
                                        )
                                        // The wide copy goes on its own coroutine, **after** the rider
                                        // has been told they are standing: it is two more round trips
                                        // for reach that may not even be needed, and the hail is live
                                        // the moment the first board holds it.
                                        hailScope.launch {
                                            runCatchingCancellable {
                                                org.ducatproject.ducat.Hailing.wideCopy(context, standing)
                                            }
                                        }
                                    }.onSuccess {
                                        withContext(Dispatchers.Main) {
                                            onPosted()
                                            onClose()
                                        }
                                    }.onFailure { e ->
                                        withContext(Dispatchers.Main) {
                                            error = e.message ?: context.getString(R.string.hail_post_failed)
                                        }
                                    }
                                    withContext(Dispatchers.Main) { busy = false }
                                }
                            },
                            enabled = !busy && from != null && to != null,
                            modifier = Modifier.fillMaxWidth().height(52.dp),
                        ) {
                            if (busy) CircularProgressIndicator(
                                Modifier.size(18.dp), strokeWidth = 2.dp)
                            else Text(stringResource(R.string.hail_post_button))
                        }
                    }

                    error?.let {
                        Spacer(Modifier.height(6.dp))
                        Text(it, color = MaterialTheme.colorScheme.error,
                            style = MaterialTheme.typography.bodySmall)
                    }
                    Text(
                        stringResource(R.string.hail_osm_privacy,
                            org.ducatproject.ducat.Units.distance(context, 1000.0)),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.outline,
                        modifier = Modifier.padding(vertical = 8.dp),
                    )
                    Spacer(Modifier.height(16.dp))
                }
            }
        }
    }
}

/** An address box: type, search, pick a candidate. Chosen state shows the
 *  picked label and clears on edit. */
@Composable
private fun AddressField(
    label: String,
    chosen: org.ducatproject.ducat.Geo.Hit?,
    onChosen: (org.ducatproject.ducat.Geo.Hit?) -> Unit,
    near: Pair<Long, Long>? = null,
    hint: String? = null,
    trailing: (@Composable () -> Unit)? = null,
) {
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    var query by remember { mutableStateOf("") }
    var hits by remember { mutableStateOf<List<org.ducatproject.ducat.Geo.Hit>>(emptyList()) }
    var searching by remember { mutableStateOf(false) }
    LaunchedEffect(chosen) { if (chosen != null) { query = chosen.label.take(48); hits = emptyList() } }

    fun search() {
        if (query.isBlank()) return
        searching = true
        scope.launch {
            hits = withContext(Dispatchers.IO) {
                org.ducatproject.ducat.Geo.search(query.trim(), near)
            }
            searching = false
        }
    }

    Row(verticalAlignment = Alignment.CenterVertically) {
        OutlinedTextField(
            value = query,
            onValueChange = { query = it; if (chosen != null) onChosen(null) },
            label = { Text(label) },
            placeholder = { hint?.let { Text(it) } },
            modifier = Modifier.weight(1f), singleLine = true,
            keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                imeAction = androidx.compose.ui.text.input.ImeAction.Search,
            ),
            keyboardActions = androidx.compose.foundation.text.KeyboardActions(
                onSearch = { search() },
            ),
            trailingIcon = {
                IconButton(onClick = { search() }, enabled = query.isNotBlank() && !searching) {
                    if (searching) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                    else Icon(Icons.Filled.Search, stringResource(R.string.hail_search))
                }
            },
        )
        trailing?.let { Spacer(Modifier.width(8.dp)); it() }
    }
    hits.forEach { h ->
        Text(
            h.label,
            style = MaterialTheme.typography.bodySmall,
            modifier = Modifier.fillMaxWidth()
                .clickable { onChosen(h); hits = emptyList() }
                .padding(vertical = 8.dp, horizontal = 4.dp),
        )
    }
}

/** Off duty: forget where this driver was watching. */
private fun forgetDuty(prefs: android.content.SharedPreferences) {
    prefs.edit()
        .remove("drive_watching").remove("drive_lat")
        .remove("drive_lon").remove("drive_box").remove("drive_taken")
        .apply()
}
