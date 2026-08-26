package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Positions
import org.ducatproject.ducat.R

/**
 * Live position, in the thread it belongs to (§15.12).
 *
 * **The ask happens where it means something.** This card exists only once a
 * `RIDE_ACCEPT` is in the thread, which is §5.2.3's gate: before the accept the
 * same stream is a stranger-tracking primitive, and after it both parties have
 * chosen each other and are about to be physically co-present anyway. There is
 * deliberately no setting for this anywhere else in the app — a standing toggle
 * would convert one moment's consent into a policy nobody remembers choosing.
 *
 * **Sharing runs while this screen is open and stops when it closes.** No
 * foreground service, no background location: the phone shares while its owner
 * is looking at the ride, which is when watching each other approach is worth
 * anything, and the moment they put it away the stream goes quiet. That is a
 * real limitation and it is the honest side of the trade — the alternative is a
 * standing background location service, which is the always-on exposure this
 * section spends its whole argument avoiding. The ride's own end
 * (`Ceremony.stopFinishedPositions`, on the poller) is the backstop that holds
 * with the phone in a pocket.
 *
 * Both directions, independently: a rider may share while the driver does not,
 * and either alone is useful.
 */
@Composable
fun PositionCard(contact: Contact) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val version by ContactStore.changes.collectAsState()
    // The gate, asked the same way the poller's bound asks it — see
    // Positions.rideIsLive for why those must be one question and not two.
    val live = remember(version, contact.personaHex) {
        Positions.rideIsLive(context, contact.personaHex)
    }
    if (!live) return
    val sharing = remember(version, contact.personaHex) {
        Positions.sharing(context, contact.personaHex)
    }
    val watching = remember(version, contact.personaHex) {
        Positions.watching(context, contact.personaHex)
    }
    if (!sharing && !watching) {
        // Nothing running: the offer to start, and nothing else. A ride with
        // no position sharing should look like a ride, not like a feature
        // somebody declined.
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            TextButton(onClick = { start(context, contact) }) {
                Text(stringResource(R.string.pos_share_mine))
            }
        }
        return
    }

    var fix by remember(contact.personaHex) { mutableStateOf<Positions.Fix?>(null) }
    // Consecutive fixes the phone could not get. Three is ~12s of trying.
    var misses by remember(contact.personaHex) { mutableStateOf(0) }
    var now by remember { mutableStateOf(System.currentTimeMillis()) }
    val uri = androidx.compose.ui.platform.LocalUriHandler.current

    // The sender's loop. Fixed cadence (§15.12): a constant heartbeat leaks
    // liveness and nothing else, where an adaptive one turns the update
    // pattern itself into a channel.
    LaunchedEffect(sharing, contact.personaHex) {
        if (!sharing) return@LaunchedEffect
        while (true) {
            grabFix(context) { at ->
                if (at == null) {
                    // **Say so.** A phone that cannot get a fix sends nothing,
                    // and the card used to go on saying "Sharing your
                    // position" regardless — the same screen claiming a thing
                    // it was not doing. One miss is ordinary (a fix arrives a
                    // beat later); a run of them is the person needing to
                    // know, because the other side is watching a dot that
                    // stopped and cannot tell whose end is at fault.
                    misses++
                } else {
                    misses = 0
                    val (lat, lon) = at
                    // Fire-and-forget onto IO: a DHT write is seconds and this
                    // callback is on the main thread.
                    Thread {
                        Positions.push(context, contact.personaHex, lat, lon, null)
                    }.start()
                }
            }
            delay(Positions.CADENCE_MS)
        }
    }

    // The receiver's loop, at the same cadence — reading faster than the other
    // side writes would only cost round trips to see the same value.
    LaunchedEffect(watching, contact.personaHex) {
        if (!watching) return@LaunchedEffect
        while (true) {
            val got = withContext(Dispatchers.IO) {
                runCatching { Positions.pull(context, contact.personaHex) }.getOrNull()
            }
            if (got != null) fix = got
            now = System.currentTimeMillis()
            delay(Positions.CADENCE_MS)
        }
    }

    Surface(
        color = MaterialTheme.colorScheme.secondaryContainer,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
            if (watching) {
                val f = fix
                val ageMs = f?.let { now - it.capturedSecs * 1000 }
                Text(
                    when {
                        f == null -> stringResource(R.string.pos_waiting, contact.displayName())
                        // **Staleness is rendered as staleness** (§15.12), never
                        // as a guessed position: a dot that keeps moving on a
                        // dead stream is the one thing a map must never draw.
                        ageMs != null && ageMs > Positions.STALE_AFTER_MS ->
                            stringResource(
                                R.string.pos_last_seen,
                                contact.displayName(),
                                (ageMs / 1000).coerceAtLeast(1),
                            )
                        else -> stringResource(R.string.pos_moving, contact.displayName())
                    },
                    style = MaterialTheme.typography.bodyMedium,
                )
                f?.let {
                    Spacer(Modifier.height(2.dp))
                    // Display-only, and said so: position MAY prompt, it MUST
                    // NOT transact (§15.12). It opens a map, it never fills in
                    // a payment.
                    val la = it.latE7 / 1e7
                    val lo = it.lonE7 / 1e7
                    // Locale.US: a comma-decimal locale mints mlat=52,52000,
                    // a URL no map can open — the same rule every coordinate
                    // URL in Geo.kt follows.
                    val url = "https://www.openstreetmap.org/?mlat=%.5f&mlon=%.5f#map=16/%.5f/%.5f"
                        .format(java.util.Locale.US, la, lo, la, lo)
                    TextButton(
                        onClick = { runCatching { uri.openUri(url) } },
                        contentPadding = PaddingValues(horizontal = 0.dp, vertical = 0.dp),
                    ) { Text(stringResource(R.string.pos_open_map)) }
                }
            }
            if (sharing) {
                if (watching) Spacer(Modifier.height(4.dp))
                Text(
                    stringResource(
                        if (misses >= 3) R.string.pos_no_fix else R.string.pos_sharing_note,
                    ),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSecondaryContainer,
                )
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (sharing) {
                    TextButton(onClick = {
                        Thread { Positions.stop(context, contact.personaHex) }.start()
                    }) { Text(stringResource(R.string.pos_stop_sharing)) }
                } else {
                    TextButton(onClick = { start(context, contact) }) {
                        Text(stringResource(R.string.pos_share_mine))
                    }
                }
            }
        }
    }
}

/** Minting a record and sealing the reference are both network work. */
private fun start(context: android.content.Context, contact: Contact) {
    if (!locationAllowed(context)) {
        // Asking for the position before asking for the permission would put
        // a dead button in front of somebody. The permission flow lives on the
        // screens that already own it; here we simply do not start.
        org.ducatproject.ducat.DucatLog.w("PositionCard", "no location permission — not sharing")
        return
    }
    Thread { Positions.start(context, contact) }.start()
}
