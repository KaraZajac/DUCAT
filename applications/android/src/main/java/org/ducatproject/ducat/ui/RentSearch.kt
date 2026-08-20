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
import androidx.compose.runtime.saveable.rememberSaveable
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
/** The chip a noun wears on the board. Short: five of them share a row. */
private fun boardChip(kind: Int): Int = when (kind) {
    Listings.KIND_VEHICLE -> R.string.board_chip_cars
    Listings.KIND_GEAR -> R.string.board_chip_gear
    Listings.KIND_SALE -> R.string.board_chip_sale
    Listings.KIND_SKILL -> R.string.board_chip_skills
    else -> R.string.board_chip_places
}

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
private enum class Stall { NoPermission, NoFix, NoNetwork }

@Composable
@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
private fun RentSearchScreen(kind: Int, onClose: () -> Unit, onOpenChat: (Contact) -> Unit) {
    // Which nouns to show. The board holds all five and one read returns all
    // of them (§16.18), so filtering here costs nothing — where asking the
    // network once per noun would cost the read five times over, and an empty
    // board is a flat twenty-one seconds each.
    var showing by rememberSaveable { mutableStateOf(kind) }
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
                // Whether this device is on the network at all, asked before
                // and after. A read of a board nobody can reach does not fail
                // — it succeeds, finds nothing, and takes its twenty-one
                // seconds doing it — so counting failures never catches this;
                // only the node's own view of itself does. The driver's map
                // has always asked this question and the search never did.
                fun attached() = runCatching {
                    uniffi.ducat_mobile.nodeStatus().publicInternetReady
                }.getOrDefault(false)
                if (!attached()) {
                    stalled = Stall.NoNetwork
                    searching = false
                    return@launch
                }
                val replied = withContext(Dispatchers.IO) {
                    runCatching {
                        Listings.search(
                            // null: everything on the board, in one pass.
                            fix.first, fix.second, null,
                            onFound = { sofar ->
                                // Each board that answers updates the list, so
                                // what is nearby appears while the ring is
                                // still being read (an empty board can take a
                                // minute).
                                results = sofar.sortedBy { it.pricePxmr }
                            },
                            onProgress = { done, total -> progress = done to total },
                        )
                    }.getOrDefault(0)
                }
                // The ring is done; whatever is here is the answer — unless
                // no board answered at all, in which case there is no answer
                // and saying "nothing listed around here" would be a
                // confident lie. Opening this a few seconds after the app
                // starts, before the node has attached, does exactly that.
                if (replied == 0 || (results.isNullOrEmpty() && !attached())) {
                    stalled = Stall.NoNetwork
                }
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
                    // One read holds every noun, so switching between them is
                    // free — and a count on each chip is the honest way to
                    // say "there are three kayaks and no plumbers near you"
                    // without making anybody wait again to find out.
                    else -> Unit
                }
                if (results != null && stalled == null) {
                    FlowRow(
                        Modifier.fillMaxWidth().padding(bottom = 8.dp),
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        Listings.KINDS.forEach { k ->
                            val n = results!!.count { it.kind.toInt() == k }
                            FilterChip(
                                selected = showing == k,
                                onClick = { showing = k },
                                label = { Text("${stringResource(boardChip(k))} $n") },
                            )
                        }
                    }
                }
                when {
                    stalled != null -> Stalled(
                        stall = stalled!!,
                        onRetry = {
                            if (stalled == Stall.NoPermission) {
                                askForLocation(context, asked) { locPerm.launch(it) }
                            } else {
                                attempt++
                            }
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
                    results!!.none { it.kind.toInt() == showing } -> Column {
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
                        items(results!!.filter { it.kind.toInt() == showing }) { info ->
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
                when (stall) {
                    Stall.NoPermission -> R.string.rent_search_needs_location
                    Stall.NoFix -> R.string.rent_search_no_fix
                    Stall.NoNetwork -> R.string.rent_search_no_network
                },
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
/**
 * The card as five nouns, for the render test.
 *
 * Worth drawing rather than reasoning about: this card read "not a vehicle"
 * as "a place" for its icon, its price unit and its category, so a bicycle
 * for sale arrived with a roof over it, priced per night, in the Whole place
 * category. Three separate wrong answers from one assumption.
 */
@Composable
@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
internal fun ListingCardsPreview() {
    androidx.compose.foundation.layout.Column(
        Modifier.padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        // Five chips with a count each, on a phone. One read holds every
        // noun, so this row is how somebody moves between them — and five
        // labels plus five numbers is exactly the width that wraps badly if
        // nobody looks.
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            listOf(2, 1, 3, 5, 0).forEachIndexed { i, n ->
                val k = Listings.KINDS[i]
                FilterChip(
                    selected = k == Listings.KIND_SALE,
                    onClick = {},
                    label = { Text("${stringResource(boardChip(k))} $n") },
                )
            }
        }
        listOf(
            Listings.KIND_PLACE to Triple("Sunny room near the park", 25_000_000_000uL, 2uL),
            Listings.KIND_VEHICLE to Triple("2019 Corolla, automatic", 40_000_000_000uL, 1uL),
            Listings.KIND_GEAR to Triple("Sea kayak, paddle included", 15_000_000_000uL, 3uL),
            Listings.KIND_SALE to Triple("Bicycle, barely ridden", 90_000_000_000uL, 4uL),
            Listings.KIND_SKILL to Triple("Electrician, 20 years", 30_000_000_000uL, 1uL),
        ).forEach { (kind, d) ->
            val (title, price, subtype) = d
            ListingCard(
                info = RentalInfo(
                    card = "ducat:card/x", kind = kind.toULong(), title = title,
                    area = "north side", cell = "u33dc", pricePxmr = price,
                    depositPxmr = 4_000_000_000uL, expiry = 1_800_000_000uL,
                    make = null, model = null, year = null, gearbox = null, fuel = null,
                    seats = null, color = null, trim = null,
                    rooms = null, sleeps = null, sizeM2 = null,
                    subtype = subtype, features = listOf("good condition"),
                ),
                busy = false, onAsk = {},
            )
        }
    }
}

@Composable
private fun ListingCard(info: RentalInfo, busy: Boolean, onAsk: () -> Unit) {
    val context = LocalContext.current
    val kind = info.kind.toInt()
    val vehicle = kind == Listings.KIND_VEHICLE
    val place = kind == Listings.KIND_PLACE
    Card(Modifier.fillMaxWidth().clickable(enabled = !busy) { onAsk() }) {
        Column(Modifier.padding(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                // The kind's own icon. A house stood in for "not a vehicle",
                // which put a roof on a kayak, a bicycle and an electrician
                // the moment the board held more than two nouns.
                Icon(listingIcon(kind), null, Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text(info.title, style = MaterialTheme.typography.titleSmall)
            }
            Spacer(Modifier.height(4.dp))
            // Per night, per day, per hour — or, for a sale, the price with
            // nothing after it, because that is the whole of it.
            val shown = Amounts.show(context, info.pricePxmr.toLong()).primary
            Text(
                if (kind == Listings.KIND_SALE) shown
                else stringResource(priceLabelShort(kind), shown),
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
                } else if (place) {
                    info.rooms?.let { add("$it") }
                    info.sleeps?.let { add("$it") }
                    info.sizeM2?.let { add("$it m²") }
                    info.subtype?.let {
                        add(stringResource(
                            if (it.toInt() == 2) R.string.rent_private_room
                            else R.string.rent_whole_place,
                        ))
                    }
                } else {
                    // This branch used to be "a place", so every one of the
                    // three new nouns rendered its category through a
                    // whole-place / private-room lookup: a bicycle in the
                    // Sport category read "Whole place".
                    info.subtype?.let { add(stringResource(categoryLabel(kind, it.toInt()))) }
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
