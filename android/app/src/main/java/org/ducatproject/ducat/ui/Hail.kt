package org.ducatproject.ducat.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material.icons.filled.MyLocation
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Search
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
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.MainActivity
import org.ducatproject.ducat.MyProfile
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
    var expanded by remember { mutableStateOf(false) }
    var sheetOpen by remember { mutableStateOf(false) }
    val context = LocalContext.current
    var cell by remember { mutableStateOf("") }
    var dest by remember { mutableStateOf("") }
    var fareXmr by remember { mutableStateOf("") }
    // Geocells (§15.12): where I am, coarsened to ~1.2 km by construction.
    var myFix by remember { mutableStateOf<Pair<Long, Long>?>(null) }
    var originCell by remember { mutableStateOf<String?>(null) }
    var destPlace by remember { mutableStateOf<org.ducatproject.ducat.PlaceStore.Place?>(null) }
    var savePlaceOpen by remember { mutableStateOf(false) }
    val locPerm = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.RequestPermission()
    ) { }
    var posted by remember { mutableStateOf<PostedHail?>(null) }
    var status by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    fun locate() {
        if (context.checkSelfPermission(android.Manifest.permission.ACCESS_FINE_LOCATION) !=
            android.content.pm.PackageManager.PERMISSION_GRANTED
        ) {
            locPerm.launch(android.Manifest.permission.ACCESS_FINE_LOCATION)
            return
        }
        grabFix(context) { fix ->
            myFix = fix
            originCell = fix?.let { (la, lo) ->
                runCatching { uniffi.ducat_mobile.geohashEncode(la, lo, 6u) }.getOrNull()
            }
            originCell?.let { cell = "geo:$it" }
            if (fix == null) error = "could not get a location fix"
        }
    }

    // The wait: the card's inbox answers when a driver claims (§16.9's
    // machinery unchanged). Polling, not a watch — a hail lives minutes.
    LaunchedEffect(posted) {
        val p = posted ?: return@LaunchedEffect
        while (true) {
            delay(3_000)
            val claimant = withContext(Dispatchers.IO) {
                runCatching {
                    Mailbox.collectClaims(context)
                    ContactStore(context).claimantOf(p.inboxKey)
                }.getOrNull()
            }
            if (claimant != null) {
                // Stewardship (§15.12): the notice is spent; clear the slot.
                withContext(Dispatchers.IO) {
                    runCatching { standPost(p.cell, p.subkey, ByteArray(0)) }
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
                MainActivity.openChat.value = claimant
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
            if (posted == null) {
                Icon(
                    if (expanded) Icons.Filled.ExpandLess else Icons.Filled.ExpandMore,
                    null, tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        val p = posted
        @Suppress("ConstantConditionIf")
        if (false && p == null && expanded) { // the form moved to HailSheet
            Spacer(Modifier.height(12.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                OutlinedTextField(
                    value = cell, onValueChange = { cell = it.take(64) },
                    label = { Text("Stand") },
                    placeholder = { Text("📍 or type a stand name") },
                    modifier = Modifier.weight(1f), singleLine = true,
                )
                Spacer(Modifier.width(8.dp))
                FilledTonalIconButton(onClick = { locate() }) {
                    Icon(Icons.Filled.MyLocation, "Use my location")
                }
            }
            Text(
                originCell?.let { "Your area: geo:$it (~1.2 km — never finer, by design)" }
                    ?: "📍 uses your area, ~1.2 km coarse. Or type a name both " +
                        "sides know, like a corner.",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.outline,
            )
            Spacer(Modifier.height(10.dp))
            // Destination: a saved place carries coordinates (so a fare can be
            // estimated); free text stays for "quote me". No geocoder, on
            // purpose — an address search would hand every destination you
            // ever type to whoever runs the server.
            val places = remember(savePlaceOpen) {
                org.ducatproject.ducat.PlaceStore(context).all()
            }
            if (places.isNotEmpty()) {
                Row(
                    Modifier.fillMaxWidth().horizontalScroll(
                        androidx.compose.foundation.rememberScrollState()
                    ),
                ) {
                    places.forEach { pl ->
                        FilterChip(
                            selected = destPlace?.name == pl.name,
                            onClick = {
                                destPlace = if (destPlace?.name == pl.name) null else pl
                                if (destPlace != null) dest = pl.name
                            },
                            label = { Text(pl.name) },
                            modifier = Modifier.padding(end = 6.dp),
                        )
                    }
                }
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                OutlinedTextField(
                    value = dest,
                    onValueChange = { dest = it.take(64); destPlace = null },
                    label = { Text("Where to") },
                    placeholder = { Text("airport, terminal B") },
                    modifier = Modifier.weight(1f), singleLine = true,
                )
                Spacer(Modifier.width(8.dp))
                TextButton(onClick = { savePlaceOpen = true }) { Text("Save here") }
            }
            // The estimate: base + per-km + per-min over great-circle × 1.3,
            // fiat-first like every cab rate card, snapshotted to piconero at
            // post (§15.11's meter rule). It seeds an offer, nothing more.
            val estimate = remember(myFix, destPlace) {
                val fix = myFix ?: return@remember null
                val d = destPlace ?: return@remember null
                val meters = uniffi.ducat_mobile.haversineM(
                    fix.first, fix.second, d.latE7, d.lonE7,
                ).toLong()
                org.ducatproject.ducat.Fare.estimate(context, meters)?.let { it to meters }
            }
            estimate?.let { (est, meters) ->
                val (fiat, pxmr) = est
                val cur = remember { Amounts.currency(context) }
                LaunchedEffect(pxmr) {
                    if (fareXmr.isBlank()) fareXmr = formatXmr(pxmr)
                }
                Text(
                    "≈ %.1f km by road · est. %s %.2f–%.2f".format(
                        meters / 1000.0 * org.ducatproject.ducat.Fare.CIRCUITY,
                        cur, fiat * 0.8, fiat * 1.2,
                    ),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.height(10.dp))
            OutlinedTextField(
                value = fareXmr, onValueChange = { fareXmr = it },
                label = { Text("Offer (XMR) — empty asks the driver to quote") },
                modifier = Modifier.fillMaxWidth(), singleLine = true,
            )
            Spacer(Modifier.height(16.dp))
            Button(
                onClick = {
                    busy = true; error = null
                    val theCell = cell.trim()
                    val theDest = dest.trim()
                    val fare = fareXmr.trim().toDoubleOrNull()
                        ?.let { (it * 1e12).toLong().toULong() }
                    kotlinx.coroutines.MainScope().launch(Dispatchers.IO) {
                        runCatching {
                            val card = Mailbox.issueCard(
                                context, MyProfile(context).name(),
                                (HAIL_TTL_SECS * 2).toULong(), purpose = "hail",
                            )
                            val destCell = destPlace?.let { d ->
                                runCatching {
                                    uniffi.ducat_mobile.geohashEncode(d.latE7, d.lonE7, 6u)
                                }.getOrNull()
                            }
                            val bytes = hailEncode(
                                HailInfo(
                                    card = card.uri,
                                    dest = theDest,
                                    farePxmr = fare,
                                    expiry = (System.currentTimeMillis() / 1000 +
                                        HAIL_TTL_SECS).toULong(),
                                    originCell = originCell,
                                    destCell = destCell,
                                )
                            )
                            val sub = (0..7).random().toUInt()
                            standPost(theCell, sub, bytes)
                            PostedHail(theCell, sub, card.inboxKey)
                        }.onSuccess {
                            posted = it
                            expanded = false
                            status = "Posted. Waiting for a driver…"
                            DucatLog.i(TAG, "hail posted at ${it.cell} subkey ${it.subkey}")
                        }.onFailure {
                            error = it.message ?: "could not post the hail"
                            DucatLog.w(TAG, "post: ${it.message}")
                        }
                        busy = false
                    }
                },
                enabled = !busy && cell.isNotBlank() && dest.isNotBlank(),
                modifier = Modifier.fillMaxWidth().height(52.dp),
            ) {
                if (busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                else Text("Post the hail")
            }
            Spacer(Modifier.height(8.dp))
            Text(
                "A hail is a mutual promise, like flagging a cab — nobody is " +
                    "made to show up. Payment happens at the end, from your " +
                    "confirm screen, never automatically.",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.outline,
            )
        } else if (p != null) {
            Spacer(Modifier.height(10.dp))
            Text("Standing at ${p.cell}",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.height(8.dp))
            LinearProgressIndicator(Modifier.fillMaxWidth())
            Spacer(Modifier.height(10.dp))
            OutlinedButton(
                onClick = {
                    val gone = p
                    posted = null; status = null
                    kotlinx.coroutines.MainScope().launch(Dispatchers.IO) {
                        runCatching { standPost(gone.cell, gone.subkey, ByteArray(0)) }
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
            }
        }
        error?.let {
            Spacer(Modifier.height(12.dp))
            Text(it, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall)
        }
    }
    }
    if (savePlaceOpen) SavePlaceDialog { savePlaceOpen = false }
    if (sheetOpen) {
        HailSheet(
            onPosted = { cell, sub, inbox ->
                posted = PostedHail(cell, sub, inbox)
                status = "Posted. Waiting for a driver…"
            },
            onClose = { sheetOpen = false },
        )
    }
}

private data class PostedHail(val cell: String, val subkey: UInt, val inboxKey: String)

/** Name where you stand; it becomes a destination chip with coordinates. */
@Composable
private fun SavePlaceDialog(onDismiss: () -> Unit) {
    val context = LocalContext.current
    var name by remember { mutableStateOf("") }
    var fix by remember { mutableStateOf<Pair<Long, Long>?>(null) }
    LaunchedEffect(Unit) { grabFix(context) { fix = it } }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Save this place") },
        text = {
            Column {
                Text(
                    if (fix != null) "Location fixed. Name it:"
                    else "Getting a fix…",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(8.dp))
                OutlinedTextField(
                    value = name, onValueChange = { name = it.take(24) },
                    label = { Text("Name (home, work, airport…)") },
                    singleLine = true,
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    fix?.let { (la, lo) ->
                        org.ducatproject.ducat.PlaceStore(context)
                            .add(org.ducatproject.ducat.PlaceStore.Place(name.trim(), la, lo))
                    }
                    onDismiss()
                },
                enabled = fix != null && name.isNotBlank(),
            ) { Text("Save") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
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
    var watching by remember { mutableStateOf<List<String>?>(null) }
    var myFix by remember { mutableStateOf<Pair<Long, Long>?>(null) }
    var notices by remember { mutableStateOf<List<SeenHail>>(emptyList()) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val locPerm = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.RequestPermission()
    ) { }
    fun driveHere() {
        if (context.checkSelfPermission(android.Manifest.permission.ACCESS_FINE_LOCATION) !=
            android.content.pm.PackageManager.PERMISSION_GRANTED
        ) {
            locPerm.launch(android.Manifest.permission.ACCESS_FINE_LOCATION)
            return
        }
        grabFix(context) { fix ->
            if (fix == null) { error = "could not get a location fix"; return@grabFix }
            myFix = fix
            runCatching {
                val home = uniffi.ducat_mobile.geohashEncode(fix.first, fix.second, 6u)
                val cells = listOf(home) +
                    uniffi.ducat_mobile.geohashNeighbors(home)
                cell = "geo:$home"
                watching = cells.map { "geo:$it" }
            }.onFailure { error = it.message }
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
                val got = withContext(Dispatchers.IO) {
                    runCatching {
                        standRead(c).mapNotNull { n ->
                            runCatching { hailDecode(n.data) }.getOrNull()?.let { h ->
                                if (h.expiry.toLong() > now) {
                                    SeenHail(
                                        c, n.subkey, h.card, h.dest,
                                        h.farePxmr?.toLong(), h.expiry.toLong(),
                                        h.originCell, h.destCell,
                                    )
                                } else null
                            }
                        }
                    }.getOrNull()
                }
                if (got != null) found[c] = got
            }
            notices = found.values.flatten().sortedByDescending { it.expiry }
            delay(4_000)
        }
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
            Button(
                onClick = { driveHere() },
                modifier = Modifier.fillMaxWidth().height(52.dp),
            ) {
                Icon(Icons.Filled.MyLocation, null, Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text("Drive here — watch my area")
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
                            n.farePxmr?.let { "offers ${formatXmr(it)} XMR" }
                                ?: "asks you to quote",
                            style = MaterialTheme.typography.bodyMedium,
                            fontFamily = FontFamily.Monospace,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        // The triage line (§16.17): how far to the fare, how
                        // long the job — from the cells, all the board knows.
                        val fix = myFix
                        val triage = remember(n) {
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
                        val mins = (n.expiry - System.currentTimeMillis() / 1000) / 60
                        Text("stands for another ${mins} min",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.outline)
                        Spacer(Modifier.height(10.dp))
                        Button(
                            onClick = {
                                busy = true; error = null
                                val theCell = n.cell
                                kotlinx.coroutines.MainScope().launch(Dispatchers.IO) {
                                    runCatching {
                                        val scanned = readContactCard(n.card)
                                        Mailbox.claimCard(context, scanned, null)
                                    }.onSuccess { rider ->
                                        // The notice is spent; clear its slot
                                        // so the next driver is not baited by
                                        // a card that cannot answer (§15.12).
                                        runCatching {
                                            standPost(theCell, n.subkey, ByteArray(0))
                                        }
                                        // The Uber moment: acceptance arrives
                                        // with a face on it — ETA from a real
                                        // route when one answers, and the car
                                        // the rider will scan the curb for.
                                        runCatching {
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
                                        error = "Someone beat you to that one."
                                    }
                                    busy = false
                                }
                            },
                            enabled = !busy,
                            modifier = Modifier.fillMaxWidth().height(44.dp),
                        ) { Text("Take this ride") }
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
    onPosted: (String, UInt, String) -> Unit, // cell, subkey, inboxKey
    onClose: () -> Unit,
) {
    val context = LocalContext.current
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    var from by remember { mutableStateOf<Pair<Long, Long>?>(null) }
    var fromLabel by remember { mutableStateOf("") }
    var toQuery by remember { mutableStateOf("") }
    var toHits by remember { mutableStateOf<List<org.ducatproject.ducat.Geo.Hit>>(emptyList()) }
    var to by remember { mutableStateOf<org.ducatproject.ducat.Geo.Hit?>(null) }
    var route by remember { mutableStateOf<org.ducatproject.ducat.Geo.Route?>(null) }
    var fareXmr by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var searching by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    val locPerm = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.RequestPermission()
    ) { ok -> if (ok) grabFix(context) { from = it; fromLabel = "My location" } }

    fun useMyLocation() {
        if (context.checkSelfPermission(android.Manifest.permission.ACCESS_FINE_LOCATION) ==
            android.content.pm.PackageManager.PERMISSION_GRANTED
        ) grabFix(context) { from = it; fromLabel = if (it != null) "My location" else "" }
        else locPerm.launch(android.Manifest.permission.ACCESS_FINE_LOCATION)
    }
    LaunchedEffect(Unit) { useMyLocation() }

    // Route + fare the moment both ends exist.
    LaunchedEffect(from, to) {
        val f = from ?: return@LaunchedEffect
        val t = to ?: return@LaunchedEffect
        route = null
        val r = withContext(Dispatchers.IO) {
            org.ducatproject.ducat.Geo.route(f.first, f.second, t.latE7, t.lonE7)
        }
        route = r
        if (r != null) {
            org.ducatproject.ducat.Fare.estimateExact(context, r.meters, r.seconds)
                ?.let { (_, pxmr) -> fareXmr = formatXmr(pxmr) }
        }
    }

    androidx.compose.ui.window.Dialog(
        onDismissRequest = onClose,
        properties = androidx.compose.ui.window.DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            Column(Modifier.fillMaxSize()) {
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    IconButton(onClick = onClose) { Icon(Icons.Filled.Close, "Close") }
                    Text("Hail a ride", style = MaterialTheme.typography.titleLarge)
                }

                Column(Modifier.padding(horizontal = 16.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        OutlinedTextField(
                            value = fromLabel,
                            onValueChange = { fromLabel = it },
                            label = { Text("Pickup") },
                            placeholder = { Text("tap 📍") },
                            modifier = Modifier.weight(1f), singleLine = true,
                            readOnly = true,
                        )
                        Spacer(Modifier.width(8.dp))
                        FilledTonalIconButton(onClick = { useMyLocation() }) {
                            Icon(Icons.Filled.MyLocation, "Use my location")
                        }
                    }
                    Spacer(Modifier.height(8.dp))
                    OutlinedTextField(
                        value = toQuery,
                        onValueChange = { toQuery = it; to = null },
                        label = { Text("Where to?") },
                        placeholder = { Text("address or place") },
                        modifier = Modifier.fillMaxWidth(), singleLine = true,
                        trailingIcon = {
                            IconButton(
                                onClick = {
                                    searching = true
                                    scope.launch {
                                        toHits = withContext(Dispatchers.IO) {
                                            org.ducatproject.ducat.Geo.search(toQuery.trim())
                                        }
                                        searching = false
                                        if (toHits.isEmpty()) error = "no matches — try adding a city"
                                    }
                                },
                                enabled = toQuery.isNotBlank() && !searching,
                            ) {
                                if (searching) CircularProgressIndicator(
                                    Modifier.size(18.dp), strokeWidth = 2.dp,
                                ) else Icon(Icons.Filled.Search, "Search")
                            }
                        },
                    )
                    toHits.takeIf { to == null }?.forEach { h ->
                        Text(
                            h.label,
                            style = MaterialTheme.typography.bodySmall,
                            modifier = Modifier.fillMaxWidth()
                                .clickable {
                                    to = h; toHits = emptyList(); toQuery = h.label.take(40)
                                    error = null
                                }
                                .padding(vertical = 8.dp, horizontal = 4.dp),
                        )
                    }
                }

                Spacer(Modifier.height(8.dp))
                RouteMap(
                    from = from,
                    to = to?.let { it.latE7 to it.lonE7 },
                    route = route?.points ?: emptyList(),
                    modifier = Modifier.weight(1f).fillMaxWidth(),
                )

                Column(Modifier.padding(16.dp)) {
                    route?.let { r ->
                        val est = org.ducatproject.ducat.Fare.estimateExact(context, r.meters, r.seconds)
                        val cur = remember { Amounts.currency(context) }
                        Text(
                            "%.1f km · ~%d min%s".format(
                                r.meters / 1000.0, r.seconds / 60,
                                est?.let { " · est. $cur %.2f–%.2f".format(it.first * 0.85, it.first * 1.15) } ?: "",
                            ),
                            style = MaterialTheme.typography.titleMedium,
                        )
                        Spacer(Modifier.height(8.dp))
                    }
                    OutlinedTextField(
                        value = fareXmr,
                        onValueChange = { fareXmr = it },
                        label = { Text("Your offer (XMR)") },
                        modifier = Modifier.fillMaxWidth(), singleLine = true,
                    )
                    Spacer(Modifier.height(10.dp))
                    Button(
                        onClick = {
                            busy = true; error = null
                            val f = from!!
                            val t = to!!
                            val fare = fareXmr.trim().toDoubleOrNull()
                                ?.let { (it * 1e12).toLong().toULong() }
                            scope.launch(Dispatchers.IO) {
                                runCatching {
                                    val oCell = uniffi.ducat_mobile.geohashEncode(f.first, f.second, 6u)
                                    val dCell = uniffi.ducat_mobile.geohashEncode(t.latE7, t.lonE7, 6u)
                                    val card = Mailbox.issueCard(
                                        context, MyProfile(context).name(),
                                        (HAIL_TTL_SECS * 2).toULong(), purpose = "hail",
                                    )
                                    val bytes = uniffi.ducat_mobile.hailEncode(
                                        uniffi.ducat_mobile.HailInfo(
                                            card = card.uri,
                                            dest = toQuery.take(64),
                                            farePxmr = fare,
                                            expiry = (System.currentTimeMillis() / 1000 +
                                                HAIL_TTL_SECS).toULong(),
                                            originCell = oCell,
                                            destCell = dCell,
                                        )
                                    )
                                    val cell = "geo:$oCell"
                                    val sub = (0..7).random().toUInt()
                                    uniffi.ducat_mobile.standPost(cell, sub, bytes)
                                    Triple(cell, sub, card.inboxKey)
                                }.onSuccess { (cell, sub, inbox) ->
                                    withContext(Dispatchers.Main) {
                                        onPosted(cell, sub, inbox)
                                        onClose()
                                    }
                                    DucatLog.i(TAG, "hail posted at $cell subkey $sub")
                                }.onFailure {
                                    error = it.message ?: "could not post the hail"
                                }
                                busy = false
                            }
                        },
                        enabled = !busy && from != null && to != null,
                        modifier = Modifier.fillMaxWidth().height(52.dp),
                    ) {
                        if (busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                        else Text("Hail 🚕")
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
                        modifier = Modifier.padding(top = 6.dp),
                    )
                }
            }
        }
    }
}
