package org.ducatproject.ducat.ui

import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import org.ducatproject.ducat.SafeImage
import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.DirectionsCar
import androidx.compose.material.icons.filled.House
import androidx.compose.foundation.horizontalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.Listings
import org.ducatproject.ducat.Publications
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

/** "Find a car", "Find help" — the heading, per noun. */
private fun boardFindTitle(kind: Int): Int = when (kind) {
    Listings.KIND_VEHICLE -> R.string.rent_find_a_car
    Listings.KIND_GEAR -> R.string.rent_find_gear
    Listings.KIND_SALE -> R.string.rent_find_sale
    Listings.KIND_SKILL -> R.string.rent_find_skill
    else -> R.string.rent_find_a_place
}

/** The chip a noun wears on the board. Short: five of them share a row. */
internal fun boardChipLabel(kind: Int): Int = when (kind) {
    Listings.KIND_VEHICLE -> R.string.board_chip_cars
    Listings.KIND_GEAR -> R.string.board_chip_gear
    Listings.KIND_SALE -> R.string.board_chip_sale
    Listings.KIND_SKILL -> R.string.board_chip_skills
    else -> R.string.board_chip_places
}

/**
 * Why a search never started. Not an error in the list — the list does not
 * exist yet — so it replaces the spinner rather than sitting above one.
 */
private enum class Stall { NoPermission, NoFix, NoNetwork }

@Composable
@OptIn(
    androidx.compose.foundation.layout.ExperimentalLayoutApi::class,
    androidx.compose.material3.ExperimentalMaterial3Api::class,
)
private fun RentSearchScreen(
    kind: Int,
    onOpenChat: (Contact) -> Unit,
    /**
     * Which nouns this screen can switch between.
     *
     * One of them means no chips: there is nothing to switch to. Three means
     * renting, where a room, a car and a kayak are one job with three nouns
     * in it and the home tiles pick which to open on.
     */
    chips: List<Int> = Listings.KINDS,
    /**
     * A caller-owned noun selection: the unified market row drives this
     * instead of the internal chips. Zero means every kind at once. Null
     * keeps the screen's own chips, exactly as before.
     */
    externalKind: Int? = null,
) {
    // Which nouns to show. The board holds all five and one read returns all
    // of them (§16.18), so filtering here costs nothing — where asking the
    // network once per noun would cost the read five times over, and an empty
    // board is a flat twenty-one seconds each.
    var showingState by rememberSaveable { mutableStateOf(kind) }
    val showing = externalKind ?: showingState
    fun kindShows(k: Int) = showing == 0 || k == showing
    val context = LocalContext.current
    var results by remember { mutableStateOf<List<RentalInfo>?>(null) }
    var busy by remember { mutableStateOf(false) }
    var searching by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }
    // Offering one of the things being looked at.
    //
    // Every one of the six home tiles searches. Listing lived only inside the
    // Renting operating mode, three taps down a drawer, which meant somebody
    // who wanted to sell a bicycle had to guess that selling is filed under
    // "Renting" — and the Marketplace tile, the one place the word "sell"
    // should have been, only ever browsed. This is the moment the intent
    // actually forms: you look at what people are asking for a kayak and
    // think, I have a kayak.
    //
    // Saveable for the same reason RentingScreen's is: a rotation should not
    // throw somebody out of a half-filled listing.
    var composing by rememberSaveable { mutableStateOf<Int?>(null) }

    // Nothing to introduce ourselves with, and about to introduce ourselves.
    // See NameGate: the name travels on the handshake, so a blank one arrives
    // as "Unnamed contact" and neither end is told.
    var intro by remember { mutableStateOf<(() -> Unit)?>(null) }
    // The listing somebody is looking at. Local: opening one costs nothing
    // and tells nobody, which is the whole reason it is a separate gesture
    // from asking.
    var opened by remember { mutableStateOf<RentalInfo?>(null) }

    NameGate(
        open = intro != null,
        onDismiss = { intro = null },
        onNamed = { val go = intro; intro = null; go?.invoke() },
    )

    // The claim a tap on "Ask about it" starts, by the card it is for. Off
    // the screen (claimOffScreen), because this one was cancelled in a way
    // no rotation showed: the scope further down is disposed with the rest
    // of the list when the listing form opens over it, so a claim out at
    // that moment finished — contact added, question sent — while `busy`,
    // remembered up here, stayed true for as long as the search was open
    // and every Ask button with it. Saveable, so the screen a rotation
    // rebuilds is the one that reads the outcome and opens the thread.
    var asking by rememberSaveable { mutableStateOf<String?>(null) }
    // Asking, once, so the card and the sheet cannot drift apart. It was
    // written inline in the card's onAsk; the sheet needs the same thing,
    // and two copies of "claim their card and say what this is about" is
    // two places for that to stop matching.
    val askAbout: (RentalInfo) -> Unit = { info ->
                          val go: () -> Unit = {
                            busy = true; error = null
                            asking = info.card
                            // Asked from this phone already: the
                            // card's reply is ours and the thread it
                            // opened is still here. The claim goes to
                            // it, and says nothing again — the
                            // question is already in it.
                            claimOffScreen(context, info.card, onFresh = { c ->
                                // This side knows the subject without
                                // being told: they tapped it.
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
                                // The claim alone opened an empty
                                // thread with a stranger: the owner
                                // got somebody arriving with nothing
                                // said, and the asker got a blank
                                // screen and had to remember which of
                                // the cars they had tapped. "Ask about
                                // it" is a question; this is the
                                // question.
                                runCatching {
                                    Mailbox.send(
                                        context, c,
                                        context.getString(
                                            R.string.rent_asking_about,
                                            isolate(info.title),
                                        ),
                                    )
                                }
                            })
                          }
                          if (nameGateNeeded(context)) intro = go else go()
    }
    val claimTick by ThreadSends.ticks.collectAsState()
    LaunchedEffect(claimTick, asking) {
        val k = asking?.let(::claimKey) ?: return@LaunchedEffect
        for (o in ThreadSends.take(k)) {
            asking = null
            when (o) {
                is ThreadSends.Outcome.Landed -> o.claimed(context)?.let(onOpenChat)
                is ThreadSends.Outcome.Failed -> {
                    DucatLog.w("RentSearch", "claim: ${o.error.message}")
                    error = context.getString(
                        // "Ask them for a new one" is the right thing to
                        // say to someone holding a scanned card and the
                        // wrong thing entirely here: asking is what they
                        // were trying to do.
                        claimFailureRes(o.error, alreadyUsed = R.string.rent_already_asked),
                    )
                }
            }
        }
        busy = asking != null && ThreadSends.inFlight(k)
    }

    // The form owns the screen while it is open, exactly as it does on the
    // Renting side — a half-filled listing over a live search behind it is
    // two jobs at once.
    //
    // In its own full-screen Dialog, and that is not decoration. ListingForm
    // scrolls internally; emitted here it would land in whatever composed
    // this, which for the home tiles is a Column that scrolls — and a
    // scrollable inside a scrollable is measured with infinite height, which
    // Compose does not warn about, it throws. The first tap on "Sell
    // something" took the whole app down.
    //
    // Returning rather than stacking. What is remembered above this line
    // survives the form; what is remembered below it — the attempt counter
    // and the effect keyed on it — is disposed and made afresh, so closing
    // the form starts the search over (see the note on `found` below, which
    // is where that restart once bit).
    composing?.let { k ->
        androidx.compose.ui.window.Dialog(
            onDismissRequest = { composing = null },
            properties = fullScreenDialogProperties(),
        ) {
            Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                ListingForm(kind = k, onDone = { composing = null })
            }
        }
        return
    }

    var stalled by remember { mutableStateOf<Stall?>(null) }
    var progress by remember { mutableStateOf<Pair<Int, Int>?>(null) }
    // §16.18.1's accepted degradation, made visible. With no chain view the
    // stamps on these listings show on signature and work alone — honest
    // boards look exactly the same, but so does a board somebody spammed
    // after cutting this reader off from its node. Read after the search,
    // from the state the search itself used; never a network call here.
    var unverified by remember { mutableStateOf(false) }
    // Bumped to start the search over; `asked` remembers that the system
    // dialog has had its turn, which is how a refusal is told apart from a
    // permission simply not requested yet.
    var attempt by remember { mutableIntStateOf(0) }
    var asked by remember { mutableStateOf(false) }
    // True from a pull until that search settles — it drives only the
    // pull indicator, so the auto-search on entry does not double up with
    // the in-list spinner row.
    var pulled by remember { mutableStateOf(false) }
    androidx.compose.runtime.LaunchedEffect(searching) {
        if (!searching) pulled = false
    }
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
                            context,
                            // null: everything on the board, in one pass.
                            fix.first, fix.second, null,
                            onFound = { sofar ->
                                // Every verified author gets a first-seen
                                // date. Stamped when a listing is in front of
                                // somebody, which is what "has been on this
                                // board a while" has to mean — and only for
                                // notices that verified, since an unsigned one
                                // never gets this far.
                                val at = System.currentTimeMillis()
                                sofar.forEach {
                                    org.ducatproject.ducat.Posters.seen(context, it.poster, at)
                                }
                                // **Not your own.** The board shows what is
                                // near you and your own posts are near you, so
                                // a seller browsing found their own record
                                // player with "Ask about it" under it — and
                                // tapping it claimed their own card, wrote
                                // themselves into their own contact list, and
                                // opened a thread where they asked themselves
                                // whether the thing was still available. It
                                // also burned the card's single reply slot,
                                // destroying the code a real buyer was about
                                // to scan. Mailbox.claimCard refuses this now
                                // whichever direction the card arrives from;
                                // this is so the question never comes up.
                                //
                                // Matched on the card rather than the poster
                                // key: a notice's poster is derived by the
                                // encoder and never held here, while the card
                                // in it is one this device issued and still
                                // has in its own registry.
                                val ours = org.ducatproject.ducat.ContactStore(context)
                                    .issuedCards().map { it.uri }.toSet()
                                // Each board that answers updates the list, so
                                // what is nearby appears while the ring is
                                // still being read (an empty board can take a
                                // minute).
                                results = sofar
                                    .filterNot { it.card in ours }
                                    .sortedBy { it.pricePxmr }
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
                unverified = !org.ducatproject.ducat.Beacons.hasChainView()
                searching = false
            }
        }
    }

    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            Column(Modifier.fillMaxSize().padding(16.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        // Per noun, and from what is *showing* rather than
                        // from what this sheet was opened on. It read "a
                        // vehicle, or else a place", which is the sixth time
                        // that assumption has turned up since the board grew
                        // to five nouns — so somebody looking for an
                        // electrician was told they were finding a place.
                        // And it was pinned to the opening kind, so tapping a
                        // chip left the heading describing the last screen.
                        stringResource(boardFindTitle(if (showing == 0) Listings.KIND_SALE else showing)),
                        style = MaterialTheme.typography.titleLarge,
                    )
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
                // Read once, and use that.
                //
                // These branches used to test `results` and then dereference
                // `results!!` — fine for the eager ones, a crash for the
                // LazyColumn, whose content lambda is invoked later, inside
                // its own measure pass. The search restarting between the two
                // (which is what closing the listing form does) put a null
                // through a `!!` that a `when` three lines up had just proved
                // non-null. Snapshotting is what makes the guard and the use
                // talk about the same value.
                val found = results
                if (unverified && !found.isNullOrEmpty() && stalled == null) {
                    Surface(
                        color = MaterialTheme.colorScheme.surfaceVariant,
                        shape = MaterialTheme.shapes.small,
                        modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp),
                    ) {
                        Text(
                            stringResource(R.string.board_unverified_note),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp),
                        )
                    }
                }
                if (externalKind == null && found != null && stalled == null && chips.size > 1) {
                    FlowRow(
                        Modifier.fillMaxWidth().padding(bottom = 8.dp),
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                        verticalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        chips.forEach { k ->
                            val n = found.count { it.kind.toInt() == k }
                            FilterChip(
                                selected = showing == k,
                                onClick = { showingState = k },
                                label = { Text(stringResource(R.string.board_chip_count, stringResource(boardChipLabel(k)), n)) },
                            )
                        }
                    }
                }
                // Reads as the chip above it: "Sell something" under For sale,
                // "Offer a skill" under Skills. The same five labels the
                // Renting screen uses, so the two ways in cannot drift apart.
                //
                // Outside the chips' condition: a screen pinned to one noun
                // has no chips and still has something to offer.
                //
                // Not while an ask is in flight. Opening the form takes this
                // function down the early return above, which disposes
                // everything remembered after it — the coroutine scope the
                // ask runs in included. The claim itself is blocking work on
                // an IO thread and completes regardless, so the thread was
                // opened; but the line that clears `busy` and the one that
                // opens the chat are after the suspension and never ran.
                // Coming back from the form found every card greyed out for
                // good, and the person never told they had already asked.
                if (found != null && stalled == null) {
                    OutlinedButton(
                        enabled = !busy,
                        onClick = { composing = if (showing == 0) Listings.KIND_SALE else showing },
                        modifier = Modifier.padding(bottom = 8.dp).height(40.dp),
                    ) {
                        Icon(Icons.Filled.Add, null, Modifier.size(18.dp))
                        Spacer(Modifier.width(8.dp))
                        Text(stringResource(listingButton(if (showing == 0) Listings.KIND_SALE else showing)))
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
                    // "Still looking" is decided on the tab you are actually
                    // looking at, not on the raw pile.
                    //
                    // This asked whether anything at all had come back, while
                    // the branch below asks whether anything of *this kind*
                    // has. Between the two, a board that answered with a
                    // bicycle while the rooms were still being read ended the
                    // search as far as the screen was concerned and put
                    // "nothing listed around here yet" up with a Try again
                    // button, mid-search. Rare enough to look like a fluke
                    // until the remembered paint (see Listings.search) made it
                    // happen on every mode switch.
                    found == null ||
                        (searching && found.none { kindShows(it.kind.toInt()) }) -> Column {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            // The brand's wait, not the platform's — see
                            // CatSpinner.
                            CatSpinner(
                                Modifier.size(22.dp),
                                tint = MaterialTheme.colorScheme.primary,
                            )
                            Spacer(Modifier.width(8.dp))
                            Text(
                                stringResource(R.string.rent_searching),
                                style = MaterialTheme.typography.bodyMedium,
                            )
                        }
                        // Something that moves. Nine boards, most of a minute
                        // each when they are empty — without a count this is a
                        // minute and a half of a screen that looks broken.
                        //
                        // A count that has stopped moving looks broken too. The
                        // ring is not the end of the search: a board that came
                        // back with every slot taken gets its ladder climbed
                        // afterwards, and during that the counter sits at "9 of
                        // 9 areas checked" beside a spinner, which reads as
                        // finished-but-hung. Say what it is doing instead.
                        progress?.let { (done, total) ->
                            Spacer(Modifier.height(8.dp))
                            // The bar under the words: measured while the ring
                            // is being read, and when the count tops out with
                            // work left (the ladder climb) it hands over to
                            // the indeterminate sweep instead of standing at
                            // full pretending to be finished.
                            DucatBar(
                                progress = if (done >= total) null
                                else done.toFloat() / total.coerceAtLeast(1),
                            )
                            Spacer(Modifier.height(6.dp))
                            Text(
                                if (done >= total) {
                                    stringResource(R.string.rent_search_closer)
                                } else {
                                    stringResource(R.string.rent_search_progress, done, total)
                                },
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                    // Empty is an answer, not the end of the road: somebody
                    // may list a car five minutes from now, and a screen whose
                    // only exit is Cancel makes you start the whole thing over
                    // to find out.
                    found.none { kindShows(it.kind.toInt()) } ->
                        androidx.compose.material3.pulltorefresh.PullToRefreshBox(
                            isRefreshing = pulled,
                            onRefresh = { pulled = true; attempt++ },
                        ) {
                            Column(
                                Modifier.fillMaxSize()
                                    .verticalScroll(
                                        androidx.compose.foundation
                                            .rememberScrollState(),
                                    ),
                            ) {
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
                        }
                    else -> androidx.compose.material3.pulltorefresh.PullToRefreshBox(
                        isRefreshing = pulled,
                        onRefresh = { pulled = true; attempt++ },
                    ) {
                        LazyColumn(Modifier.fillMaxSize()) {
                        if (searching) {
                            item {
                                Row(verticalAlignment = Alignment.CenterVertically) {
                                    CatSpinner(
                                        Modifier.size(14.dp),
                                        tint = MaterialTheme.colorScheme.primary,
                                    )
                                    Spacer(Modifier.width(8.dp))
                                    Text(
                                        stringResource(R.string.rent_still_looking),
                                        style = MaterialTheme.typography.labelSmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                                // The same bar as the empty state, kept while
                                // results are already up: what is underfoot
                                // landed first, and this is the rest of the
                                // ring still arriving.
                                progress?.let { (done, total) ->
                                    Spacer(Modifier.height(6.dp))
                                    DucatBar(
                                        progress = if (done >= total) null
                                        else done.toFloat() / total.coerceAtLeast(1),
                                        modifier = Modifier.fillMaxWidth().height(4.dp),
                                    )
                                }
                                Spacer(Modifier.height(8.dp))
                            }
                        }
                        items(found.filter { kindShows(it.kind.toInt()) }) { info ->
                            ListingCard(
                                info = info,
                                busy = busy,
                                onOpen = { opened = info },
                                onAsk = { askAbout(info) },
                            )
                            Spacer(Modifier.height(10.dp))
                        }
                        }
                    }
                }
            }
        }
    // Opened, not asked. The sheet closes on its own the moment the ask
    // starts, so the busy state and the errors stay on the one screen that
    // was already showing them.
    opened?.let { info ->
        ListingSheet(
            info = info,
            busy = busy,
            onDismiss = { opened = null },
            onAsk = { opened = null; askAbout(info) },
        )
    }
    }

/**
 * The Marketplace mode's first tab: the board, for the one noun it is about.
 *
 * The same search the tiles open, minus the chrome a sheet needs — no Cancel,
 * because a tab is left by tapping another tab, and no noun chips, because
 * this mode is about one noun. The board read underneath is unchanged: your
 * own coarse cell first, then the eight around it (§15.12).
 */
@Composable
fun MarketBrowse(onOpenChat: (Contact) -> Unit) {
    // §16.18.2's two honest axes: WHERE (near / worldwide) and WHAT. One
    // flat row for WHAT whose chips follow the scope, because that is what
    // the boards underneath actually hold: a neighbourhood's board carries
    // kayaks and the town paper side by side (so near-me shows the kinds
    // plus one Digital chip — local pub notices carry no category), while
    // a worldwide board IS a category (so the six shelves replace the
    // kinds, which have nowhere to stand without a place).
    // The browse remembers where you were looking across launches, not
    // just rotations: somebody shopping worldwide news all week should
    // not re-pick it every open. Plain prefs — a shelf choice is not a
    // secret.
    val browsePrefs = LocalContext.current
        .getSharedPreferences("ducat_browse", android.content.Context.MODE_PRIVATE)
    var scope by androidx.compose.runtime.saveable.rememberSaveable {
        androidx.compose.runtime.mutableStateOf(browsePrefs.getInt("scope", 0))
    }
    var what by androidx.compose.runtime.saveable.rememberSaveable {
        // 0 all · 1..5 kinds · 6 digital
        androidx.compose.runtime.mutableStateOf(browsePrefs.getInt("what", 0))
    }
    var cat by androidx.compose.runtime.saveable.rememberSaveable {
        androidx.compose.runtime.mutableStateOf(
            browsePrefs.getString("cat", null) ?: "news",
        )
    }
    var myLang by androidx.compose.runtime.saveable.rememberSaveable {
        androidx.compose.runtime.mutableStateOf(browsePrefs.getBoolean("my_lang", true))
    }
    androidx.compose.runtime.LaunchedEffect(scope, what, cat, myLang) {
        browsePrefs.edit()
            .putInt("scope", scope)
            .putInt("what", what)
            .putString("cat", cat)
            .putBoolean("my_lang", myLang)
            .apply()
    }
    androidx.compose.foundation.layout.Column(
        androidx.compose.ui.Modifier.fillMaxSize(),
    ) {
        androidx.compose.foundation.layout.Row(
            androidx.compose.ui.Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
            horizontalArrangement = androidx.compose.foundation.layout.Arrangement.spacedBy(6.dp),
        ) {
            FilterChip(
                selected = scope == 0,
                onClick = { scope = 0 },
                label = { Text(stringResource(R.string.market_near_me)) },
            )
            FilterChip(
                selected = scope == 1,
                onClick = { scope = 1 },
                label = { Text(stringResource(R.string.market_worldwide)) },
            )
        }
        androidx.compose.foundation.layout.Row(
            androidx.compose.ui.Modifier
                .horizontalScroll(androidx.compose.foundation.rememberScrollState())
                .padding(horizontal = 16.dp),
            horizontalArrangement = androidx.compose.foundation.layout.Arrangement.spacedBy(6.dp),
        ) {
            if (scope == 0) {
                listOf(
                    0 to R.string.market_what_all,
                    Listings.KIND_SALE to boardChipLabel(Listings.KIND_SALE),
                    Listings.KIND_PLACE to boardChipLabel(Listings.KIND_PLACE),
                    Listings.KIND_VEHICLE to boardChipLabel(Listings.KIND_VEHICLE),
                    Listings.KIND_GEAR to boardChipLabel(Listings.KIND_GEAR),
                    Listings.KIND_SKILL to boardChipLabel(Listings.KIND_SKILL),
                    6 to R.string.market_what_digital,
                ).forEach { (k, res) ->
                    FilterChip(
                        selected = what == k,
                        onClick = { what = k },
                        label = { Text(stringResource(res)) },
                    )
                }
            } else {
                Publications.MARKET_CATEGORIES.forEach { slug ->
                    FilterChip(
                        selected = cat == slug,
                        onClick = { cat = slug },
                        label = { Text(marketCategoryLabel(slug)) },
                    )
                }
            }
        }
        if (scope == 1) {
            val lang = java.util.Locale.getDefault()
            androidx.compose.foundation.layout.Row(
                androidx.compose.ui.Modifier.padding(horizontal = 16.dp),
            ) {
                FilterChip(
                    selected = myLang,
                    onClick = { myLang = !myLang },
                    label = {
                        Text(
                            if (myLang) lang.getDisplayLanguage(lang)
                            else stringResource(R.string.market_all_langs),
                            style = MaterialTheme.typography.labelSmall,
                        )
                    },
                )
            }
            WorldwideShelf(cat, myLang)
            return@Column
        }
        if (what == 6) {
            LocalShelf()
            return@Column
        }
        androidx.compose.runtime.key(Unit) {
            RentSearchScreen(
                kind = Listings.KIND_SALE,
                onOpenChat = onOpenChat,
                externalKind = what,
            )
        }
    }
}

@Composable
private fun MarketNearMe(onOpenChat: (Contact) -> Unit) {
    RentSearchScreen(
        kind = Listings.KIND_SALE,
        onOpenChat = onOpenChat,
        chips = Listings.SALE_KINDS,
    )
}

/**
 * Hire help's board: people, and what they charge.
 *
 * One noun, so no chips — the same shape as Marketplace, pointed at somebody
 * to do a job rather than something to buy.
 */
@Composable
fun HireBrowse(onOpenChat: (Contact) -> Unit) {
    RentSearchScreen(
        kind = Listings.KIND_SKILL,
        onOpenChat = onOpenChat,
        chips = Listings.SKILL_KINDS,
    )
}

/**
 * Renting's board: a room, a car, a kayak.
 *
 * The one mode that keeps its chips, because three home tiles lead into it
 * and each wants to land on a different noun. [initial] is which.
 */
@Composable
fun RentBrowse(initial: Int, onOpenChat: (Contact) -> Unit) {
    RentSearchScreen(
        kind = initial,
        onOpenChat = onOpenChat,
        chips = Listings.RENT_KINDS,
    )
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
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            // One of each, which is what the cards below are — a count that
            // disagrees with the list under it is a picture nobody can check
            // anything against.
            Listings.KINDS.forEach { k ->
                FilterChip(
                    selected = false,
                    onClick = {},
                    label = { Text(stringResource(R.string.board_chip_count, stringResource(boardChipLabel(k)), 1)) },
                )
            }
        }
        listOf(
            Triple(Listings.KIND_PLACE, "Sunny room near the park", 25_000_000_000L)
                to (2uL to listOf("wifi", "own bathroom")),
            Triple(Listings.KIND_VEHICLE, "2019 Corolla, automatic", 40_000_000_000L)
                to (1uL to listOf("child seat")),
            Triple(Listings.KIND_GEAR, "Sea kayak, paddle included", 15_000_000_000L)
                to (3uL to listOf("two seats")),
            Triple(Listings.KIND_SALE, "Bicycle, barely ridden", 90_000_000_000L)
                to (4uL to listOf("54cm frame", "new tyres")),
            Triple(Listings.KIND_SKILL, "Electrician, 20 years", 30_000_000_000L)
                to (1uL to listOf("rewiring", "certificates")),
        ).forEach { (spec, extra) ->
            val (kind, title, price) = spec
            val (subtype, features) = extra
            ListingCard(
                info = RentalInfo(
                    // A preview, so an author nobody has seen before — which
                    // is exactly what these cards would say on a real board.
                    poster = "",
                    card = "ducat:card/x", kind = kind.toULong(), title = title,
                    area = "north side", cell = "u33dc", pricePxmr = price.toULong(),
                    // From the kind's own deal, like a real notice. Every card
                    // used to carry the same flat figure, so five prices from
                    // 15 to 90 all claimed an identical stake — which is the
                    // one thing on these cards a reader might check, and it
                    // was checkable only against itself.
                    depositPxmr = org.ducatproject.ducat.Stakes
                        .stakeFor(Listings.dealFor(kind), price).toULong(),
                    expiry = 1_800_000_000uL,
                    make = null, model = null, year = null, gearbox = null, fuel = null,
                    seats = null, color = null, trim = null,
                    rooms = null, sleeps = null, sizeM2 = null,
                    subtype = subtype, features = features, quantity = 1uL,
                ),
                busy = false, onAsk = {}, onOpen = {},
            )
        }
    }
}

/**
 * The one line of specifics under a listing's title, in reading order.
 *
 * Lifted out of [ListingCard] so the sheet says exactly the same thing:
 * two renderings of one listing that disagreed would be the board's own
 * bytes described two ways.
 */
@Composable
private fun listingSpecs(info: RentalInfo): List<String> {
    val kind = info.kind.toInt()
    val vehicle = kind == Listings.KIND_VEHICLE
    val place = kind == Listings.KIND_PLACE
    return buildList {
        if (vehicle) {
            info.year?.let { add(it.toString()) }
            info.make?.let { add(isolate(it)) }
            info.model?.let { add(isolate(it)) }
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
            info.trim?.let { add(isolate(it)) }
            info.seats?.let {
                add(pluralStringResource(R.plurals.rent_seats_n, it.toInt(), it.toInt()))
            }
        } else if (place) {
            info.rooms?.let {
                add(pluralStringResource(R.plurals.rent_rooms_n, it.toInt(), it.toInt()))
            }
            info.sleeps?.let { add(stringResource(R.string.rent_sleeps_n, it.toInt())) }
            info.sizeM2?.let { add(stringResource(R.string.rent_size_m2, it.toInt())) }
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
        addAll(info.features.map { isolate(it) })
        // Only when there is more than one. Somebody deciding whether
        // to ask wants to know they are not competing for the last
        // one — and for the listing that *is* one thing, which is
        // nearly all of them, saying so would be noise.
        if (info.quantity > 1uL) {
            add(stringResource(R.string.rent_n_available, info.quantity.toLong()))
        }
    }
}

/**
 * One listing, opened.
 *
 * Local and free: nothing here touches the network, because everything it
 * shows arrived with the board read. The one button that costs anything is
 * the one that says so.
 */
@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
private fun ListingSheet(
    info: RentalInfo,
    busy: Boolean,
    onAsk: () -> Unit,
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    androidx.compose.material3.ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            Modifier
                .fillMaxWidth()
                .verticalScroll(androidx.compose.foundation.rememberScrollState())
                .padding(start = 20.dp, end = 20.dp, bottom = 28.dp),
        ) {
            val shot = remember(info.thumb) {
                info.thumb?.let { SafeImage.fromBytes(it, SafeImage.MESSAGE_PIXELS) }
            }
            if (shot != null) {
                Image(
                    shot.asImageBitmap(),
                    null,
                    Modifier.fillMaxWidth().height(240.dp)
                        .clip(MaterialTheme.shapes.medium),
                    contentScale = ContentScale.Crop,
                )
                Spacer(Modifier.height(12.dp))
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(listingIcon(info.kind.toInt()), null, Modifier.size(20.dp))
                Spacer(Modifier.width(8.dp))
                Text(
                    isolate(info.title),
                    style = MaterialTheme.typography.titleMedium,
                    modifier = Modifier.weight(1f),
                )
            }
            Spacer(Modifier.height(6.dp))
            Text(
                run {
                    val shown = Amounts.show(context, info.pricePxmr.toLong()).primary
                    if (info.kind.toInt() == Listings.KIND_SALE) shown
                    else stringResource(priceLabelShort(info.kind.toInt()), shown)
                },
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
            val specs = listingSpecs(info)
            if (specs.isNotEmpty()) {
                Spacer(Modifier.height(6.dp))
                Text(
                    specs.joinToString(" \u00b7 "),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (info.area.isNotBlank()) {
                Spacer(Modifier.height(2.dp))
                Text(
                    isolate(info.area),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.height(16.dp))
            // Says what pressing it does, because it is not free: it opens a
            // conversation with somebody and spends the code they posted.
            Text(
                stringResource(R.string.rent_ask_note),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(6.dp))
            Button(
                enabled = !busy,
                onClick = onAsk,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(stringResource(R.string.rent_ask_about_it))
            }
        }
    }
}

@Composable
private fun ListingCard(
    info: RentalInfo,
    busy: Boolean,
    onAsk: () -> Unit,
    onOpen: () -> Unit,
) {
    val context = LocalContext.current
    val kind = info.kind.toInt()
    val vehicle = kind == Listings.KIND_VEHICLE
    val place = kind == Listings.KIND_PLACE
    // Tapping the card *looks*; only the button asks.
    //
    // The whole card used to call onAsk, which claims the seller's
    // claim-once card and sends them a message. That was defensible while a
    // listing was words — you tapped it because you had decided. It stopped
    // being defensible the moment listings grew photographs, because a
    // picture is a thing people tap to see better, and the cost of being
    // wrong is a stranger's card burnt and a conversation they did not
    // start. Looking is now local, free and reversible; asking is still one
    // deliberate press.
    Card(Modifier.fillMaxWidth().clickable(enabled = !busy) { onOpen() }) {
        Column {
            // §16.18.3's thumbnail, straight off the board — it arrived with
            // the notice, so drawing it costs nothing more than the read
            // that is already done. A listing without one is not missing
            // anything: it simply starts at its title.
            val shot = remember(info.thumb) {
                info.thumb?.let { SafeImage.fromBytes(it, SafeImage.MESSAGE_PIXELS) }
            }
            if (shot != null) {
                Image(
                    shot.asImageBitmap(),
                    null,
                    Modifier.fillMaxWidth().height(160.dp),
                    contentScale = ContentScale.Crop,
                )
            }
        Column(Modifier.padding(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                // The kind's own icon. A house stood in for "not a vehicle",
                // which put a roof on a kayak, a bicycle and an electrician
                // the moment the board held more than two nouns.
                Icon(listingIcon(kind), null, Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                // Off a public board, so written by whoever posted it. The
                // wire refuses the overrides; strong right-to-left text is
                // honest and still has to be kept inside its own line.
                Text(
                    isolate(info.title),
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 2,
                    overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
                // How long this listing's author has been on this phone's
                // boards. Nobody owns a board slot, so a copied listing with a
                // swapped card is indistinguishable by content — the author is
                // the only difference there is, and a substitution shows up as
                // one that turned up today.
                //
                // Said only when it is the reassuring direction. A "new"
                // badge on every honest first listing would train people to
                // ignore the one that mattered.
                val settled = remember(info.poster) {
                    org.ducatproject.ducat.Posters
                        .settled(context, info.poster, System.currentTimeMillis())
                }
                if (settled) {
                    Spacer(Modifier.width(8.dp))
                    Text(
                        stringResource(R.string.rent_poster_known),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.primary,
                    )
                }
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
            // Numbers say what they count. A car's line ended "Hybrid LE · 5"
            // and a room's began "1 · 2 · 28 m²" — the seats, bedrooms and
            // sleeping places, which the form labels and the card did not.
            //
            // And what the owner typed is isolated from what this phone
            // says: the make, model, trim and tags come off a public board,
            // and a right-to-left one beside a localised "automatic" would
            // otherwise reorder the whole line.
            val specs = listingSpecs(info)
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
                    isolate(info.area),
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
}
