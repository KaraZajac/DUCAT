package org.ducatproject.ducat.ui

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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.MainActivity
import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.RideStore
import org.ducatproject.ducat.formatXmr
import uniffi.ducat_mobile.HailInfo
import uniffi.ducat_mobile.hailDecode
import uniffi.ducat_mobile.hailEncode
import uniffi.ducat_mobile.readContactCard
import uniffi.ducat_mobile.standPost
import uniffi.ducat_mobile.standRead

private const val TAG = "Hail"

/** How long a posted hail stands before the board should ignore it. */
private const val HAIL_TTL_SECS = 15L * 60

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
private fun clearOwnSlot(board: String, subkey: UInt, myCardUri: String) {
    runCatching {
        val held = standRead(board).firstOrNull { it.subkey == subkey } ?: return
        val notice = runCatching { hailDecode(held.data) }.getOrNull() ?: return
        if (notice.card == myCardUri) standPost(board, subkey, ByteArray(0))
    }
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
            standPost(name, free, p.notice)
            val landed = standRead(name).firstOrNull { it.subkey == free }
                ?.let { runCatching { hailDecode(it.data) }.getOrNull() }
            if (landed?.card == p.card) {
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
@Composable
fun HailCard() {
    var sheetOpen by remember { mutableStateOf(false) }
    var driverFound by remember { mutableStateOf<org.ducatproject.ducat.Contact?>(null) }
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
                    clearOwnSlot(p.cell, p.subkey, p.card)
                    rides.clear()
                }
                DucatLog.i(TAG, "hail expired unclaimed")
                status = "Nobody took your hail before it expired."
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
                    clearOwnSlot(p.cell, p.subkey, p.card)
                    rides.clear()
                }
                val d = ContactStore(context).all().firstOrNull { it.personaHex == claimant }
                val who = d?.displayName() ?: "a driver"
                // The stranger's car (§16.9 fields 210–212): what the rider
                // scans the curb for. Claims — the check is the bumper.
                val car = listOfNotNull(d?.carColor, d?.carModel).joinToString(" ")
                    .ifBlank { null }
                val ride = listOfNotNull(car, d?.plate?.let { "plate $it" })
                    .joinToString(", ").ifBlank { null }
                DucatLog.i(TAG, "hail claimed by $who")
                status = "$who took your hail" +
                    (ride?.let { " — look for a $it" } ?: "") + ". ETA in the chat."
                posted = null
                // The ceremony (Ceremony.kt): the stranger's face, car and
                // plate get the whole screen, and the chat is one tap away
                // rather than an abrupt teleport into it.
                driverFound = d
                break
            }
        }
    }

    Spacer(Modifier.height(12.dp))
    Card(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
    Column(Modifier.fillMaxWidth().padding(16.dp)) {
        Row(
            Modifier.fillMaxWidth().clickable { if (posted == null) sheetOpen = true },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("🚕", style = MaterialTheme.typography.headlineSmall)
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Text("Hail a ride", style = MaterialTheme.typography.titleMedium)
                Text(
                    if (posted != null) "Hail standing — waiting for a driver"
                    else "Post where you're going to a stand drivers watch.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

        }

        val p = posted
        if (p != null) {
            Spacer(Modifier.height(10.dp))
            Text("Standing at ${p.cell.removePrefix("geo:").substringBefore("-")}",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.height(8.dp))
            LinearProgressIndicator(Modifier.fillMaxWidth())
            Spacer(Modifier.height(10.dp))
            OutlinedButton(
                onClick = {
                    val gone = p
                    posted = null; status = null
                    hailScope.launch {
                        clearOwnSlot(gone.cell, gone.subkey, gone.card)
                        rides.clear()
                    }
                },
                modifier = Modifier.fillMaxWidth(),
            ) { Text("Take it down") }
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
                    ) { Text("Post it again") }
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
    if (sheetOpen) {
        HailSheet(
            onPosted = {
                // The sheet persisted the ride before announcing it; the
                // store is the truth even if this card was rebuilt meanwhile.
                posted = rides.load()?.asPosted()
                expired = false
                status = "Posted. Waiting for a driver…"
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
)

private fun RideStore.PostedRide.asPosted() =
    PostedHail(board, subkey, inboxKey, cardUri, expiry, notice)


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
    var watching by remember { mutableStateOf<List<String>?>(null) }
    var myFix by remember { mutableStateOf<Pair<Long, Long>?>(null) }
    var notices by remember { mutableStateOf<List<SeenHail>>(emptyList()) }
    var selected by remember { mutableStateOf<SeenHail?>(null) }
    var coverage by remember { mutableStateOf<LongArray?>(null) }
    // The net's size is the driver's call: one cell where hails are thick,
    // 5×5 where a fare is worth chasing across a rural county.
    val rangePrefs = remember {
        context.getSharedPreferences("ducat_contacts", android.content.Context.MODE_PRIVATE)
    }
    var range by remember { mutableStateOf(rangePrefs.getInt("drive_range", 3)) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
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
            if (fix == null) { error = "could not get a location fix"; return@grabFix }
            myFix = fix
            runCatching {
                val home = uniffi.ducat_mobile.geohashEncode(fix.first, fix.second, 6u)
                val ring1 = uniffi.ducat_mobile.geohashNeighbors(home)
                val cells = when (range) {
                    1 -> listOf(home)
                    5 -> (listOf(home) + ring1 +
                        ring1.flatMap { uniffi.ducat_mobile.geohashNeighbors(it) })
                        .distinct()
                    else -> listOf(home) + ring1
                }
                // The outer box of a contiguous block of cells is the net,
                // drawn on the map so coverage is seen instead of guessed.
                var latLo = Long.MAX_VALUE; var latHi = Long.MIN_VALUE
                var lonLo = Long.MAX_VALUE; var lonHi = Long.MIN_VALUE
                cells.forEach { c ->
                    val b = uniffi.ducat_mobile.geohashBounds(c)
                    latLo = minOf(latLo, b[0]); latHi = maxOf(latHi, b[1])
                    lonLo = minOf(lonLo, b[2]); lonHi = maxOf(lonHi, b[3])
                }
                coverage = longArrayOf(latLo, latHi, lonLo, lonHi)
                cell = "geo:$home"
                watching = cells.map { "geo:$it" }
            }.onFailure { error = it.message }
        }
    }

    val takeHailShared: (SeenHail) -> Unit = { taken ->
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
                        runCatchingCancellable {
                            val me = org.ducatproject.ducat.MyProfile(context)
                            val fix = myFix
                            val o = taken.originCell?.let {
                                uniffi.ducat_mobile.geohashCenter(it)
                            }
                            val etaMin = if (fix != null && o != null) {
                                org.ducatproject.ducat.Geo.route(
                                    fix.first, fix.second, o[0], o[1],
                                )?.let { (it.seconds / 60).coerceAtLeast(1) }
                                    ?: (uniffi.ducat_mobile.haversineM(
                                        fix.first, fix.second, o[0], o[1],
                                    ).toLong() * 2 / 1000).coerceAtLeast(1)
                            } else null
                            val car = listOfNotNull(me.carColor(), me.carModel())
                                .joinToString(" ").ifBlank { null }
                            val msg = buildString {
                                append("🚕 On my way")
                                etaMin?.let { append(" — ETA ~$it min") }
                                append(".")
                                car?.let { append(" Look for a $it") }
                                me.plate()?.let { append(", plate $it") }
                                if (car != null || me.plate() != null) append(".")
                            }
                            Mailbox.send(
                                context, rider, msg,
                                org.ducatproject.ducat.PersonaStore(context).personaHex(),
                            )
                        }
                        DucatLog.i(TAG, "took a hail to ${taken.dest}")
                        withContext(Dispatchers.Main) {
                            MainActivity.openChat.value = rider.personaHex
                        }
                    }.onFailure {
                        DucatLog.w(TAG, "claim: ${it.message}")
                        withContext(Dispatchers.Main) {
                            error = "Someone beat you to that one."
                        }
                    }
                    withContext(Dispatchers.Main) { busy = false }
                }
    }

    LaunchedEffect(watching) {
        val cells = watching ?: return@LaunchedEffect
        // Round-robin three boards per tick: nine sequential force-refreshed
        // reads every cycle is a phone doing nothing else.
        var at = 0
        val found = HashMap<String, List<SeenHail>>()
        while (true) {
            val batch = cells.drop(at).take(3).ifEmpty { cells.take(3) }
            at = (at + 3) % cells.size
            val now = System.currentTimeMillis() / 1000
            for (c in batch) {
                // §15.12's overflow ladder: sweep shards from 0. Claims and
                // expiry empty the low shards first, so one quiet shard is
                // not the end of the ladder — stop only after two in a row,
                // which costs a quiet cell one extra read.
                val got = withContext(Dispatchers.IO) {
                    runCatchingCancellable {
                        val all = mutableListOf<SeenHail>()
                        var quiet = 0
                        for (shard in 0u until uniffi.ducat_mobile.maxStandShards()) {
                            val name = uniffi.ducat_mobile.standShardName(c, shard)
                            val live = standRead(name).mapNotNull { n ->
                                runCatching { hailDecode(n.data) }.getOrNull()?.let { h ->
                                    if (h.expiry.toLong() > now) {
                                        SeenHail(
                                            name, n.subkey, h.card, h.dest,
                                            h.farePxmr?.toLong(), h.expiry.toLong(),
                                            h.originCell, h.destCell,
                                        )
                                    } else null
                                }
                            }
                            if (live.isEmpty()) {
                                if (++quiet >= 2) break
                            } else {
                                quiet = 0
                                all += live
                            }
                        }
                        all
                    }.getOrNull()
                }
                if (got != null) found[c] = got
            }
            // Cells whose reads keep failing hold their last good sweep, so
            // filter by expiry again here or a stale notice lingers forever.
            val cutoff = System.currentTimeMillis() / 1000
            notices = found.values.flatten()
                .filter { it.expiry > cutoff }
                // A migrating notice can stand in two slots for a moment;
                // the card is the identity, so one pin, not two.
                .distinctBy { it.card }
                .sortedByDescending { it.expiry }
            delay(4_000)
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
                    Text("On duty", style = MaterialTheme.typography.titleMedium)
                    val n = watching?.size ?: 0
                    Text(
                        when {
                            n >= 25 -> "wide net — ~6 km across"
                            n >= 9 -> "watching ~3.5 km across"
                            else -> "just your area, ~1.2 km"
                        },
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.outline,
                    )
                }
                OutlinedButton(onClick = { watching = null; notices = emptyList() }) {
                    Text("Stop")
                }
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
                    Surface(
                        color = MaterialTheme.colorScheme.surface.copy(alpha = 0.85f),
                        shape = MaterialTheme.shapes.medium,
                        modifier = Modifier.align(Alignment.TopCenter).padding(12.dp),
                    ) {
                        Text(
                            "No hails standing — they'll pin here.",
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
                                    } ?: "quote me",
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
        Text("Driving", style = MaterialTheme.typography.headlineSmall)
        Spacer(Modifier.height(4.dp))
        Text(
            "Watch a stand; take a hail. Taking one opens a conversation with " +
                "the rider — quote there, meet there, bill at the end like any " +
                "ride.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(16.dp))

        if (watching == null) {
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                listOf(1 to "Just here", 3 to "Nearby", 5 to "Wide").forEachIndexed { i, (v, label) ->
                    SegmentedButton(
                        selected = range == v,
                        onClick = { range = v; rangePrefs.edit().putInt("drive_range", v).apply() },
                        shape = SegmentedButtonDefaults.itemShape(i, 3),
                        icon = {},
                    ) { Text(label) }
                }
            }
            Text(
                when (range) {
                    1 -> "~1.2 km — dense city blocks"
                    5 -> "~6 km across — rural or slow nights"
                    else -> "~3.5 km across — the usual"
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
                    Text("Getting your location…")
                } else {
                    Icon(Icons.Filled.MyLocation, null, Modifier.size(18.dp))
                    Spacer(Modifier.width(8.dp))
                    Text("Drive here — watch my area")
                }
            }
            Spacer(Modifier.height(8.dp))
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(
                value = cell, onValueChange = { cell = it.take(64) },
                label = { Text("Stand") },
                placeholder = { Text("stand name, or 📍 above") },
                modifier = Modifier.weight(1f), singleLine = true,
                enabled = watching == null,
            )
            Spacer(Modifier.width(8.dp))
            if (watching == null) {
                Button(onClick = { watching = listOf(cell.trim()) },
                    enabled = cell.isNotBlank()) { Text("Watch") }
            } else {
                OutlinedButton(onClick = {
                    watching = null; notices = emptyList()
                }) { Text("Stop") }
            }
        }
        watching?.let { w ->
            if (w.size > 1) {
                Text(
                    "Watching your cell and its 8 neighbours — " +
                        w.first().removePrefix("geo:") + " +8",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.outline,
                )
            }
        }

        if (watching != null) {
            Spacer(Modifier.height(16.dp))
            if (notices.isEmpty()) {
                Text(
                    "Nothing on the board. Checking every few seconds…",
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
                                "offers ${shown.primary}" +
                                    (shown.secondary?.let { s -> " · $s" } ?: "")
                            } ?: "asks you to quote",
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
                                    parts += "pickup ~%.1f km".format(m.toLong() / 1000.0)
                                }
                                val d = n.destCell?.let {
                                    uniffi.ducat_mobile.geohashCenter(it)
                                }
                                if (o != null && d != null) {
                                    val m = uniffi.ducat_mobile.haversineM(
                                        o[0], o[1], d[0], d[1])
                                    parts += "trip ~%.1f km".format(
                                        m.toLong() / 1000.0 * org.ducatproject.ducat.Fare.CIRCUITY)
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
                            Text("stands for another ${mins} min",
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.outline)
                        }
                        Spacer(Modifier.height(10.dp))
                        Row {
                        OutlinedButton(
                            onClick = { selected = n },
                            modifier = Modifier.weight(1f).height(44.dp),
                        ) { Text("View job") }
                        Spacer(Modifier.width(8.dp))
                        Button(
                            onClick = {
                                busy = true; error = null
                                val theCell = n.cell
                                hailScope.launch {
                                    runCatchingCancellable {
                                        val scanned = readContactCard(n.card)
                                        Mailbox.claimCard(context, scanned, null, asDriver = true)
                                    }.onSuccess { rider ->
                                        // The notice is spent; clear its slot
                                        // so the next driver is not baited by
                                        // a card that cannot answer (§15.12) —
                                        // but only if it still holds the
                                        // notice we claimed.
                                        clearOwnSlot(theCell, n.subkey, n.card)
                                        // The Uber moment: acceptance arrives
                                        // with a face on it — ETA from a real
                                        // route when one answers, and the car
                                        // the rider will scan the curb for.
                                        runCatchingCancellable {
                                            val me = org.ducatproject.ducat.MyProfile(context)
                                            val fix = myFix
                                            val o = n.originCell?.let {
                                                uniffi.ducat_mobile.geohashCenter(it)
                                            }
                                            val etaMin = if (fix != null && o != null) {
                                                org.ducatproject.ducat.Geo.route(
                                                    fix.first, fix.second, o[0], o[1],
                                                )?.let { (it.seconds / 60).coerceAtLeast(1) }
                                                    ?: (uniffi.ducat_mobile.haversineM(
                                                        fix.first, fix.second, o[0], o[1],
                                                    ).toLong() * 2 / 1000).coerceAtLeast(1)
                                            } else null
                                            val car = listOfNotNull(
                                                me.carColor(), me.carModel(),
                                            ).joinToString(" ").ifBlank { null }
                                            val msg = buildString {
                                                append("🚕 On my way")
                                                etaMin?.let { append(" — ETA ~$it min") }
                                                append(".")
                                                car?.let { append(" Look for a $it") }
                                                me.plate()?.let { append(", plate $it") }
                                                if (car != null || me.plate() != null) append(".")
                                            }
                                            Mailbox.send(
                                                context, rider, msg,
                                                org.ducatproject.ducat.PersonaStore(context)
                                                    .personaHex(),
                                            )
                                        }.onFailure {
                                            DucatLog.w(TAG, "intro message: ${it.message}")
                                        }
                                        DucatLog.i(TAG, "took a hail to ${n.dest}")
                                        withContext(Dispatchers.Main) {
                                            MainActivity.openChat.value = rider.personaHex
                                        }
                                    }.onFailure {
                                        DucatLog.w(TAG, "claim: ${it.message}")
                                        withContext(Dispatchers.Main) {
                                            error = "Someone beat you to that one."
                                        }
                                    }
                                    withContext(Dispatchers.Main) { busy = false }
                                }
                            },
                            enabled = !busy,
                            modifier = Modifier.weight(1f).height(44.dp),
                        ) { Text("Take it") }
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
    onTake: (SeenHail) -> Unit,
    onClose: () -> Unit,
) {
    val context = LocalContext.current
    var route by remember { mutableStateOf<org.ducatproject.ducat.Geo.Route?>(null) }
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
                    IconButton(onClick = onClose) { Icon(Icons.Filled.Close, "Close") }
                    Text("The job", style = MaterialTheme.typography.titleLarge)
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
                                "to the pickup: %.1f km · ~%d min".format(
                                    it.first / 1000.0, (it.second / 60).coerceAtLeast(1)),
                                style = MaterialTheme.typography.bodyMedium,
                            )
                        }
                        trip?.let {
                            Text(
                                "the ride: %.1f km · ~%d min".format(
                                    it.first / 1000.0, (it.second / 60).coerceAtLeast(1)),
                                style = MaterialTheme.typography.bodyMedium,
                            )
                        }
                        Spacer(Modifier.height(8.dp))
                        n.farePxmr?.let { fare ->
                            val shown = Amounts.show(context, fare)
                            Text(
                                "offer: ${shown.primary}" +
                                    (shown.secondary?.let { s -> " · $s" } ?: ""),
                                style = MaterialTheme.typography.titleMedium,
                            )
                            trip?.let { t ->
                                val (_, uberDriver, _) = org.ducatproject.ducat.Fare
                                    .competitors(t.first, t.second)
                                val cur = Amounts.currency(context)
                                Text(
                                    ("you keep all of it — a rideshare would pay " +
                                        "~$cur %.0f for this trip").format(uberDriver),
                                    style = MaterialTheme.typography.labelMedium,
                                    color = MaterialTheme.ducat.settled,
                                )
                            }
                        } ?: Text("rider asks you to quote",
                            style = MaterialTheme.typography.titleMedium)
                    } ?: Row(verticalAlignment = Alignment.CenterVertically) {
                        CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                        Spacer(Modifier.width(8.dp))
                        Text("Routing the job…", style = MaterialTheme.typography.bodySmall)
                    }
                    Spacer(Modifier.height(14.dp))
                    Button(
                        onClick = { onTake(n) },
                        enabled = !busy,
                        modifier = Modifier.fillMaxWidth().height(52.dp),
                    ) { Text("Take this ride 🚕") }
                    Spacer(Modifier.height(6.dp))
                    OutlinedButton(
                        onClick = onClose,
                        modifier = Modifier.fillMaxWidth().height(44.dp),
                    ) { Text("Pass") }
                }
            }
        }
    }
}

private data class SeenHail(
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
    var from by remember { mutableStateOf<org.ducatproject.ducat.Geo.Hit?>(null) }
    var to by remember { mutableStateOf<org.ducatproject.ducat.Geo.Hit?>(null) }
    var route by remember { mutableStateOf<org.ducatproject.ducat.Geo.Route?>(null) }
    var routing by remember { mutableStateOf(false) }
    var fareXmr by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    val locPerm = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.RequestPermission()
    ) { ok ->
        if (ok) grabFix(context) { f ->
            from = f?.let { org.ducatproject.ducat.Geo.Hit("My location", it.first, it.second) }
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
                from = f?.let { org.ducatproject.ducat.Geo.Hit("My location", it.first, it.second) }
                if (f == null) error = "could not get a location fix"
            }
        } else locPerm.launch(android.Manifest.permission.ACCESS_FINE_LOCATION)
    }
    LaunchedEffect(Unit) { useMyLocation() }

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
            error = "no route found between those points"
        } else {
            error = null
            org.ducatproject.ducat.Fare.estimateExact(context, r.meters, r.seconds)
                ?.let { (_, pxmr) -> fareXmr = formatXmr(pxmr) }
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
                    IconButton(onClick = onClose) { Icon(Icons.Filled.Close, "Close") }
                    Text("Hail a ride", style = MaterialTheme.typography.titleLarge)
                }

                Column(Modifier.padding(horizontal = 16.dp)) {
                    AddressField(
                        label = "Pickup",
                        chosen = from,
                        onChosen = { from = it },
                        near = from?.let { it.latE7 to it.lonE7 },
                        hint = if (locating) "Locating…" else null,
                        trailing = {
                            FilledTonalIconButton(onClick = { useMyLocation() }) {
                                Icon(Icons.Filled.MyLocation, "Use my location")
                            }
                        },
                    )
                    Spacer(Modifier.height(8.dp))
                    AddressField(
                        label = "Where to?",
                        chosen = to,
                        onChosen = { to = it },
                        // Bias toward the pickup: a business name means the
                        // one near you, not the one in another hemisphere.
                        near = from?.let { it.latE7 to it.lonE7 },
                        hint = "address or business name",
                    )

                    if (routing) {
                        Spacer(Modifier.height(16.dp))
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                            Spacer(Modifier.width(8.dp))
                            Text("Finding the route…",
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
                            "%.1f km · ~%d min%s".format(
                                r.meters / 1000.0, r.seconds / 60,
                                est?.let { " · $cur %.2f".format(it.first) } ?: "",
                            ),
                            style = MaterialTheme.typography.titleMedium,
                        )
                        est?.let { (fiat, _) ->
                            val (uber, uberDriver, taxi) =
                                org.ducatproject.ducat.Fare.competitors(r.meters, r.seconds)
                            Text(
                                ("rideshare ~$cur %.0f · taxi ~$cur %.0f — your driver " +
                                    "keeps all of it (a rideshare pays theirs ~$cur %.0f)")
                                    .format(uber, taxi, uberDriver),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.ducat.settled,
                            )
                        }
                        Spacer(Modifier.height(8.dp))
                        OutlinedTextField(
                            value = fareXmr,
                            onValueChange = { fareXmr = it },
                            label = { Text("Your offer (XMR)") },
                            modifier = Modifier.fillMaxWidth(), singleLine = true,
                        )
                        Spacer(Modifier.height(10.dp))
                        Button(
                            onClick = onClick@{
                                // A comma-decimal keyboard types "0,004", and
                                // a minus or garbage must refuse loudly here —
                                // posting "quote me" instead of the typed
                                // offer is a silent repricing.
                                val fareText = fareXmr.trim().replace(',', '.')
                                var fare: ULong? = null
                                if (fareText.isNotEmpty()) {
                                    val v = fareText.toDoubleOrNull()
                                    if (v == null || v <= 0.0) {
                                        error = "The offer must be a number above zero — " +
                                            "or leave it blank to ask for a quote."
                                        return@onClick
                                    }
                                    fare = (v * 1e12).toLong().toULong()
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
                                        val oCell = uniffi.ducat_mobile.geohashEncode(
                                            f.latE7, f.lonE7, 6u)
                                        val dCell = uniffi.ducat_mobile.geohashEncode(
                                            t.latE7, t.lonE7, 6u)
                                        val card = Mailbox.issueCard(
                                            context, MyProfile(context).name(),
                                            (HAIL_TTL_SECS * 2).toULong(), purpose = "hail",
                                        )
                                        val expiry = System.currentTimeMillis() / 1000 +
                                            HAIL_TTL_SECS
                                        val bytes = uniffi.ducat_mobile.hailEncode(
                                            uniffi.ducat_mobile.HailInfo(
                                                card = card.uri,
                                                dest = destText,
                                                farePxmr = fare,
                                                expiry = expiry.toULong(),
                                                originCell = oCell,
                                                destCell = dCell,
                                            )
                                        )
                                        // §15.12's overflow ladder, with a
                                        // read-back: two riders can race for
                                        // the same free slot and the DHT keeps
                                        // whoever wrote last, silently. Only a
                                        // slot that reads back holding our
                                        // card counts as placed; a lost one
                                        // just continues the walk.
                                        val base = "geo:$oCell"
                                        var placed: Pair<String, UInt>? = null
                                        ladder@ for (shard in 0u until uniffi.ducat_mobile.maxStandShards()) {
                                            val name = uniffi.ducat_mobile.standShardName(base, shard)
                                            val nowS = System.currentTimeMillis() / 1000
                                            val taken = standRead(name).mapNotNull { n ->
                                                runCatching { hailDecode(n.data) }.getOrNull()
                                                    ?.takeIf { it.expiry.toLong() > nowS }
                                                    ?.let { n.subkey }
                                            }.toSet()
                                            for (free in 0u..7u) {
                                                if (free in taken) continue
                                                // A post error means the slot
                                                // went to someone else.
                                                if (runCatching {
                                                        uniffi.ducat_mobile.standPost(name, free, bytes)
                                                    }.isFailure) continue
                                                val held = runCatching {
                                                    standRead(name)
                                                        .firstOrNull { it.subkey == free }
                                                        ?.let { hailDecode(it.data) }
                                                        ?.card == card.uri
                                                }.getOrDefault(false)
                                                if (held) {
                                                    placed = name to free
                                                    break@ladder
                                                }
                                            }
                                        }
                                        val (board, sub) = placed
                                            ?: error("every stand shard is full here — try again shortly")
                                        // Persist before announcing: the Home
                                        // card rehydrates from this record, so
                                        // the hail survives even if this sheet
                                        // is already gone.
                                        RideStore(context).save(
                                            RideStore.PostedRide(
                                                board = board, subkey = sub,
                                                inboxKey = card.inboxKey,
                                                cardUri = card.uri, expiry = expiry,
                                                notice = bytes,
                                            )
                                        )
                                        DucatLog.i(TAG, "hail posted at $board subkey $sub")
                                    }.onSuccess {
                                        withContext(Dispatchers.Main) {
                                            onPosted()
                                            onClose()
                                        }
                                    }.onFailure { e ->
                                        withContext(Dispatchers.Main) {
                                            error = e.message ?: "could not post the hail"
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
                            else Text("Hail 🚕")
                        }
                    }

                    error?.let {
                        Spacer(Modifier.height(6.dp))
                        Text(it, color = MaterialTheme.colorScheme.error,
                            style = MaterialTheme.typography.bodySmall)
                    }
                    Text(
                        "Search, route and map use OpenStreetMap's servers — the " +
                            "one place DUCAT sends location off-device. Drivers see " +
                            "only ~1 km areas.",
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
                    else Icon(Icons.Filled.Search, "Search")
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
