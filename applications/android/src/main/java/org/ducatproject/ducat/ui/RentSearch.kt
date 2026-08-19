package org.ducatproject.ducat.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.DirectionsCar
import androidx.compose.material.icons.filled.House
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.Listings
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.R
import uniffi.ducat_mobile.RentalInfo

/**
 * The seeker's half of §16.18, on the personal Home screen.
 *
 * Looking for a car or a place to stay is a moment, like hailing — not a job
 * you run all day — so it lives beside the balance rather than behind a
 * mode, exactly as the hail card does. Two chips, because the two searches
 * are genuinely different questions and a single list of everything nearby
 * would answer neither.
 *
 * What it reads is the 3×3 neighbourhood of boards around where you are,
 * which at §16.18's precision 5 is roughly a metro area — the granularity
 * chosen because people travel to collect a car or a set of keys.
 */
@Composable
fun RentSearchCard(
    onOpenChat: (Contact) -> Unit,
    /**
     * Which search is open, or none — hoisted so the home screen's two tiles
     * can start it. The card of chips this used to draw is gone: the tiles are
     * the way in, and a second one underneath was the same button twice.
     */
    kindState: MutableState<Int?> = remember { mutableStateOf(null) },
) {
    var kind by kindState
    kind?.let {
        RentSearchScreen(kind = it, onClose = { kind = null }, onOpenChat = onOpenChat)
    }
}

/**
 * Why a search never started. Not an error in the list — the list does not
 * exist yet — so it replaces the spinner rather than sitting above one.
 */
private enum class Stall { NoPermission, NoFix }

@Composable
private fun RentSearchScreen(kind: Int, onClose: () -> Unit, onOpenChat: (Contact) -> Unit) {
    val context = LocalContext.current
    var results by remember { mutableStateOf<List<RentalInfo>?>(null) }
    var busy by remember { mutableStateOf(false) }
    var searching by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }
    var stalled by remember { mutableStateOf<Stall?>(null) }
    var progress by remember { mutableStateOf<Pair<Int, Int>?>(null) }
    // Bumped to start the search over; `asked` remembers that the system
    // dialog has had its turn, which is how a refusal is told apart from a
    // permission simply not requested yet.
    var attempt by remember { mutableIntStateOf(0) }
    var asked by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val perm = android.Manifest.permission.ACCESS_FINE_LOCATION
    val locPerm = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.RequestPermission(),
    ) { granted ->
        asked = true
        if (granted) attempt++ else { stalled = Stall.NoPermission; searching = false }
    }

    LaunchedEffect(kind, attempt) {
        // Boards are laid out by area, so this search cannot begin without a
        // rough position — and asking is this screen's job. It used to assume
        // some earlier screen had asked, which meant anyone who came here
        // before ever hailing watched a spinner that could never finish.
        if (context.checkSelfPermission(perm) !=
            android.content.pm.PackageManager.PERMISSION_GRANTED
        ) {
            locPerm.launch(perm)
            return@LaunchedEffect
        }
        stalled = null
        searching = true
        progress = null
        results = null
        grabFix(context) { fix ->
            if (fix == null) {
                stalled = Stall.NoFix
                searching = false
                return@grabFix
            }
            scope.launch {
                withContext(Dispatchers.IO) {
                    runCatching {
                        Listings.search(
                            fix.first, fix.second, kind,
                            onFound = { sofar ->
                                // Each board that answers updates the list, so
                                // what is nearby appears while the ring is
                                // still being read (an empty board can take a
                                // minute).
                                results = sofar.sortedBy { it.pricePxmr }
                            },
                            onProgress = { done, total -> progress = done to total },
                        )
                    }
                }
                // The ring is done; whatever is here is the answer.
                if (results == null) results = emptyList()
                searching = false
            }
        }
    }

    androidx.compose.ui.window.Dialog(
        onDismissRequest = onClose,
        properties = fullScreenDialogProperties(),
    ) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            Column(Modifier.fillMaxSize().padding(16.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        stringResource(
                            if (kind == Listings.KIND_VEHICLE) R.string.rent_find_a_car
                            else R.string.rent_find_a_place,
                        ),
                        style = MaterialTheme.typography.titleLarge,
                    )
                    Spacer(Modifier.weight(1f))
                    TextButton(onClick = onClose) { Text(stringResource(R.string.rent_cancel)) }
                }
                Text(
                    stringResource(R.string.rent_search_note),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                error?.let {
                    Spacer(Modifier.height(8.dp))
                    Text(it, color = MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.bodySmall)
                }
                Spacer(Modifier.height(12.dp))
                when {
                    // A stall replaces the spinner instead of joining it: the
                    // screen said "looking at the boards around you" and
                    // "waiting for a location fix" at the same time, forever,
                    // and both halves of that were untrue.
                    stalled != null -> Stalled(
                        stall = stalled!!,
                        onRetry = {
                            if (stalled == Stall.NoFix) attempt++
                            else askForLocation(context, asked) { locPerm.launch(it) }
                        },
                    )
                    results == null || (results!!.isEmpty() && searching) -> Column {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                            Spacer(Modifier.width(8.dp))
                            Text(
                                stringResource(R.string.rent_searching),
                                style = MaterialTheme.typography.bodyMedium,
                            )
                        }
                        // Something that moves. Nine boards, most of a minute
                        // each when they are empty — without a count this is a
                        // minute and a half of a screen that looks broken.
                        progress?.let { (done, total) ->
                            Spacer(Modifier.height(6.dp))
                            Text(
                                stringResource(R.string.rent_search_progress, done, total),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                    // Empty is an answer, not the end of the road: somebody
                    // may list a car five minutes from now, and a screen whose
                    // only exit is Cancel makes you start the whole thing over
                    // to find out.
                    results!!.isEmpty() -> Column {
                        Text(
                            stringResource(R.string.rent_none_found),
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Spacer(Modifier.height(12.dp))
                        OutlinedButton(onClick = { attempt++ }) {
                            Text(stringResource(R.string.rent_search_retry))
                        }
                    }
                    else -> LazyColumn(Modifier.fillMaxSize()) {
                        if (searching) {
                            item {
                                Row(verticalAlignment = Alignment.CenterVertically) {
                                    CircularProgressIndicator(
                                        Modifier.size(12.dp), strokeWidth = 2.dp,
                                    )
                                    Spacer(Modifier.width(8.dp))
                                    Text(
                                        stringResource(R.string.rent_still_looking),
                                        style = MaterialTheme.typography.labelSmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                                Spacer(Modifier.height(8.dp))
                            }
                        }
                        items(results!!) { info ->
                            ListingCard(
                                info = info,
                                busy = busy,
                                onAsk = {
                                    busy = true; error = null
                                    scope.launch {
                                        val r = withContext(Dispatchers.IO) {
                                            runCatching {
                                                val card = uniffi.ducat_mobile
                                                    .readContactCard(info.card)
                                                val c = Mailbox.claimCard(context, card, null)
                                                // This side knows the subject
                                                // without being told: they
                                                // tapped it.
                                                org.ducatproject.ducat.Enquiries.remember(
                                                    context, c.personaHex,
                                                    org.ducatproject.ducat.Enquiries.About(
                                                        title = info.title,
                                                        pricePxmr = info.pricePxmr.toLong(),
                                                        depositPxmr = info.depositPxmr.toLong(),
                                                        kind = info.kind.toInt(),
                                                    ),
                                                )
                                                // Say what this is about.
                                                //
                                                // The claim alone opened an
                                                // empty thread with a stranger:
                                                // the owner got somebody
                                                // arriving with nothing said,
                                                // and the asker got a blank
                                                // screen and had to remember
                                                // which of the cars they had
                                                // tapped. "Ask about it" is a
                                                // question; this is the
                                                // question.
                                                runCatching {
                                                    Mailbox.send(
                                                        context, c,
                                                        context.getString(
                                                            R.string.rent_asking_about,
                                                            info.title,
                                                        ),
                                                        org.ducatproject.ducat
                                                            .PersonaStore(context).personaHex(),
                                                    )
                                                }
                                                c
                                            }
                                        }
                                        busy = false
                                        r.onSuccess { onOpenChat(it); onClose() }
                                            .onFailure {
                                                DucatLog.w("RentSearch", "claim: ${it.message}")
                                                error = context.getString(
                                                    // "Ask them for a new one"
                                                    // is the right thing to say
                                                    // to someone holding a
                                                    // scanned card and the
                                                    // wrong thing entirely
                                                    // here: asking is what
                                                    // they were trying to do.
                                                    claimFailureRes(
                                                        it,
                                                        alreadyUsed = R.string.rent_already_asked,
                                                    ),
                                                )
                                            }
                                    }
                                },
                            )
                            Spacer(Modifier.height(10.dp))
                        }
                    }
                }
            }
        }
    }
}

/**
 * The search could not start, said plainly and with the way out attached.
 */
@Composable
private fun Stalled(stall: Stall, onRetry: () -> Unit) {
    Column {
        Text(
            stringResource(
                if (stall == Stall.NoPermission) R.string.rent_search_needs_location
                else R.string.rent_search_no_fix,
            ),
            style = MaterialTheme.typography.bodyMedium,
        )
        Spacer(Modifier.height(12.dp))
        Button(onClick = onRetry) {
            Text(
                stringResource(
                    if (stall == Stall.NoPermission) R.string.rent_search_allow
                    else R.string.rent_search_retry,
                ),
            )
        }
    }
}

/**
 * One listing as a stranger sees it: everything the board carries, and a way
 * to ask. The address is not here because it is not on the board — the card
 * below is what turns this into a conversation where it can be.
 */
@Composable
private fun ListingCard(info: RentalInfo, busy: Boolean, onAsk: () -> Unit) {
    val context = LocalContext.current
    val vehicle = info.kind.toInt() == Listings.KIND_VEHICLE
    Card(Modifier.fillMaxWidth().clickable(enabled = !busy) { onAsk() }) {
        Column(Modifier.padding(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(
                    if (vehicle) Icons.Filled.DirectionsCar else Icons.Filled.House,
                    null, Modifier.size(18.dp),
                )
                Spacer(Modifier.width(8.dp))
                Text(info.title, style = MaterialTheme.typography.titleSmall)
            }
            Spacer(Modifier.height(4.dp))
            Text(
                stringResource(
                    if (vehicle) R.string.rent_per_day_short else R.string.rent_per_night_short,
                    Amounts.show(context, info.pricePxmr.toLong()).primary,
                ),
                style = MaterialTheme.typography.bodyMedium,
            )
            if (info.depositPxmr > 0uL) {
                Text(
                    stringResource(
                        R.string.rent_stake_short,
                        Amounts.show(context, info.depositPxmr.toLong()).primary,
                    ),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            val specs = buildList {
                if (vehicle) {
                    info.year?.let { add(it.toString()) }
                    info.make?.let { add(it) }
                    info.model?.let { add(it) }
                    info.gearbox?.let {
                        add(stringResource(
                            if (it.toInt() == 1) R.string.rent_manual else R.string.rent_automatic,
                        ))
                    }
                    info.fuel?.let {
                        add(stringResource(when (it.toInt()) {
                            2 -> R.string.rent_diesel
                            3 -> R.string.rent_electric
                            4 -> R.string.rent_hybrid
                            else -> R.string.rent_petrol
                        }))
                    }
                    info.trim?.let { add(it) }
                    info.seats?.let { add("$it") }
                } else {
                    info.rooms?.let { add("$it") }
                    info.sleeps?.let { add("$it") }
                    info.sizeM2?.let { add("$it m²") }
                    info.subtype?.let {
                        add(stringResource(
                            if (it.toInt() == 2) R.string.rent_private_room
                            else R.string.rent_whole_place,
                        ))
                    }
                }
                addAll(info.features)
            }
            if (specs.isNotEmpty()) {
                Spacer(Modifier.height(4.dp))
                Text(
                    specs.joinToString(" · "),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (info.area.isNotBlank()) {
                Text(
                    info.area,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.height(8.dp))
            Button(enabled = !busy, onClick = onAsk) {
                Text(stringResource(R.string.rent_ask_about_it))
            }
        }
    }
}
