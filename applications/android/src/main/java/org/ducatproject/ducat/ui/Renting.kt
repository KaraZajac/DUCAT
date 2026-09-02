package org.ducatproject.ducat.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.MenuBook
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Backpack
import androidx.compose.material.icons.filled.Build
import androidx.compose.material.icons.filled.DirectionsCar
import androidx.compose.material.icons.filled.LocalOffer
import androidx.compose.material.icons.filled.Remove
import androidx.compose.material.icons.filled.House
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.Listings
import org.ducatproject.ducat.R
import org.ducatproject.ducat.Stakes
import org.ducatproject.ducat.standStale
import org.json.JSONArray
import org.json.JSONObject

/**
 * The renting mode (§15.11): what this device has to rent out.
 *
 * The owner's half of §16.18. Personal mode is where someone *looks* for a
 * car or a place; this is where someone *has* one. Same split as the taxi:
 * a rider hails from the wallet, a driver runs a shift from a mode.
 *
 * A listing is mostly a form, and the form is the design work — what a
 * stranger can search on has to be asked here, and what would let them walk
 * up to the door must be asked here too and then kept off the board.
 */
/**
 * The words a kind wears.
 *
 * Kept together rather than scattered through the form, because five kinds
 * with three labels each is exactly the shape that drifts: a screen ends up
 * calling a kayak a vehicle in one place and gear in another.
 */
internal fun listingButton(kind: Int): Int = when (kind) {
    Listings.KIND_VEHICLE -> R.string.rent_list_a_vehicle
    Listings.KIND_GEAR -> R.string.rent_list_gear
    Listings.KIND_SALE -> R.string.rent_list_sale
    Listings.KIND_SKILL -> R.string.rent_list_skill
    else -> R.string.rent_list_a_place
}

private fun listingFormTitle(kind: Int): Int = when (kind) {
    Listings.KIND_VEHICLE -> R.string.rent_form_vehicle
    Listings.KIND_GEAR -> R.string.rent_form_gear
    Listings.KIND_SALE -> R.string.rent_form_sale
    Listings.KIND_SKILL -> R.string.rent_form_skill
    else -> R.string.rent_form_place
}

/** The same, abbreviated, for a card in a list. */
internal fun priceLabelShort(kind: Int): Int = when (kind) {
    Listings.KIND_VEHICLE, Listings.KIND_GEAR -> R.string.rent_per_day_short
    Listings.KIND_SKILL -> R.string.rent_per_hour_short
    else -> R.string.rent_per_night_short
}

/** Per night, per day, per hour, or once — §16.18's unit-follows-the-kind. */
internal fun priceLabel(kind: Int): Int = when (kind) {
    Listings.KIND_VEHICLE, Listings.KIND_GEAR -> R.string.rent_per_day
    Listings.KIND_SKILL -> R.string.rent_per_hour
    Listings.KIND_SALE -> R.string.rent_price_once
    else -> R.string.rent_per_night
}

/** The icon a kind wears, for a list that mixes all five. */
internal fun listingIcon(kind: Int) = when (kind) {
    Listings.KIND_VEHICLE -> Icons.Filled.DirectionsCar
    Listings.KIND_GEAR -> Icons.Filled.Backpack
    Listings.KIND_SALE -> Icons.Filled.LocalOffer
    Listings.KIND_SKILL -> Icons.Filled.Build
    else -> Icons.Filled.House
}

/**
 * The tags that will actually go on the board.
 *
 * The wire takes eight tags of sixteen characters, and this used to cut them
 * to fit in silence at the moment of posting: somebody who typed
 * "skis winter alpine" published "skis winter alpi" — mid-word, on a public
 * board, and only ever saw it afterwards on somebody else's screen. The cut
 * has to happen, so it happens here and the form shows the result while there
 * is still a chance to write it differently.
 */
internal fun boardTags(typed: String): List<String> =
    typed.split(',').map { it.trim().take(16) }.filter { it.isNotEmpty() }.take(8)

/**
 * A category, per kind.
 *
 * Small flat sets rather than a tree, and the numbers must match core's
 * `rental_subtype_top` exactly — a form offering a category the wire refuses
 * would produce a listing nobody can read back.
 */
internal fun categoryLabel(kind: Int, n: Int): Int = when (kind) {
    Listings.KIND_SALE -> when (n) {
        1 -> R.string.cat_goods
        2 -> R.string.cat_furniture
        3 -> R.string.cat_tools
        4 -> R.string.cat_sport
        5 -> R.string.cat_garden
        6 -> R.string.cat_electronics
        7 -> R.string.cat_music
        8 -> R.string.cat_vehicle
        else -> R.string.cat_other
    }
    Listings.KIND_GEAR -> when (n) {
        1 -> R.string.cat_sport
        2 -> R.string.cat_tools
        3 -> R.string.cat_outdoor
        4 -> R.string.cat_party
        else -> R.string.cat_other
    }
    Listings.KIND_SKILL -> when (n) {
        1 -> R.string.cat_electrical
        2 -> R.string.cat_plumbing
        3 -> R.string.cat_carpentry
        4 -> R.string.cat_painting
        5 -> R.string.cat_cleaning
        6 -> R.string.cat_moving
        7 -> R.string.cat_gardening
        8 -> R.string.cat_repairs
        9 -> R.string.cat_tutoring
        10 -> R.string.cat_care
        11 -> R.string.cat_tech
        else -> R.string.cat_other
    }
    else -> R.string.cat_other
}

@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
/**
 * The things you are offering, and the buttons to offer more.
 *
 * Scoped to a set of kinds, because the board is shared and the *jobs* are
 * not: selling a bicycle is Marketplace's business and letting a room is
 * Renting's, and a screen that mixed them would show each mode the other
 * one's work. One screen serves both — the only difference between them is
 * which nouns they are about.
 */
@Composable
fun RentingScreen(kinds: List<Int> = Listings.KINDS) {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val listings = remember(version, kinds) {
        Listings.all(context).filter { it.optInt("kind") in kinds }
    }
    // Saveable: this is which form is open, and losing it on a rotation
    // threw somebody out of a half-filled listing back to the list.
    var composing by rememberSaveable { mutableStateOf<Int?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    composing?.let { kind ->
        ListingForm(kind = kind, onDone = { composing = null })
        return
    }

    Column(Modifier.fillMaxSize()) {
        // Five nouns on one board. Wrapped rather than a row, because five
        // buttons across does not fit the phone a market stall actually owns.
        FlowRow(
            Modifier.fillMaxWidth().padding(16.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            kinds.forEach { k ->
                // Tonal, not filled. These are peers — up to five of them — and
                // five filled buttons is five primary actions shouting at once
                // across the top third of an otherwise empty screen. Tonal
                // keeps them the tab's obvious business without any one of them
                // claiming to be the answer, which none of them is: which noun
                // you have is not a thing the app can guess.
                FilledTonalButton(
                    onClick = { composing = k },
                    modifier = Modifier.height(44.dp),
                ) {
                    Icon(listingIcon(k), null, Modifier.size(18.dp))
                    Spacer(Modifier.width(8.dp))
                    Text(stringResource(listingButton(k)))
                }
            }
            // Selling a publication is selling too. A seller should not need
            // to know the physical/digital ontology to find the button —
            // this one walks them into the press room, where listing lives.
            if (kinds.contains(Listings.KIND_SALE)) {
                FilledTonalButton(
                    onClick = { marketListYours() },
                    modifier = Modifier.height(44.dp),
                ) {
                    Icon(Icons.Filled.MenuBook, null, Modifier.size(18.dp))
                    Spacer(Modifier.width(8.dp))
                    Text(stringResource(R.string.market_list_pub))
                }
            }
        }
        error?.let {
            Text(
                it, Modifier.padding(horizontal = 16.dp),
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall,
            )
        }
        // else, not a return: this is inside Column's lambda, and returning
        // out of it non-locally leaves Compose unwinding to a group marker
        // that is no longer on the stack. See Items.kt, where the same line
        // crashed the app on every fresh till.
        if (listings.isEmpty()) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text(
                    stringResource(R.string.rent_nothing_listed),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else {
            LazyColumn(Modifier.fillMaxSize().padding(horizontal = 16.dp)) {
                items(listings) { o ->
                    MyListingCard(
                        o = o,
                        onPost = {
                            scope.launch {
                                error = null
                                withContext(Dispatchers.IO) {
                                    runCatching { Listings.post(context, o.optString("id")) }
                                }
                                    // **False is a failure too.** post() throws
                                    // when it cannot reach a node, and returns
                                    // false when every shard of the board is
                                    // taken — and only the throw was being
                                    // reported. A seller pressed Post on a busy
                                    // board, nothing appeared, and the listing
                                    // was not up.
                                    .onSuccess {
                                        if (!it) error = context.getString(R.string.rent_board_full)
                                    }
                                    .onFailure { error = moneyFailure(context, it) }
                            }
                        },
                        onStop = {
                            scope.launch {
                                withContext(Dispatchers.IO) {
                                    runCatching { Listings.unpost(context, o.optString("id")) }
                                }.onFailure { error = moneyFailure(context, it) }
                            }
                        },
                        onDelete = {
                            scope.launch {
                                withContext(Dispatchers.IO) {
                                    if (o.optString("board").isNotBlank()) {
                                        runCatching { Listings.unpost(context, o.optString("id")) }
                                    }
                                    Listings.remove(context, o.optString("id"))
                                }
                            }
                        },
                        onQuantity = { n ->
                            scope.launch {
                                // The store is an encrypted blob read whole and
                                // written back; off the main thread like every
                                // other write on this screen.
                                withContext(Dispatchers.IO) {
                                    Listings.setQuantity(context, o.optString("id"), n)
                                }
                                // Straight onto the board rather than at the next
                                // six-hourly refresh: the count is what a reader
                                // is deciding on, and "two left" that is really
                                // none is the reason to have it at all.
                                if (o.optString("board").isNotBlank()) {
                                    withContext(Dispatchers.IO) {
                                        runCatching { Listings.post(context, o.optString("id")) }
                                    }
                                        .onSuccess {
                                            if (!it) error = context.getString(R.string.rent_board_full)
                                        }
                                        .onFailure { error = moneyFailure(context, it) }
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

@Composable
private fun MyListingCard(
    o: JSONObject,
    onPost: () -> Unit,
    onStop: () -> Unit,
    onDelete: () -> Unit,
    onQuantity: (Long) -> Unit,
) {
    val context = LocalContext.current
    // Posted, and not so long ago that the notice has run out.
    //
    // "board" is set when a listing goes up and cleared when it is taken
    // down, and nothing in between ever cleared it — so a notice that had
    // simply expired still read "Live on the board near you". The poller
    // re-posts every six hours against a 24-hour expiry, which keeps this
    // true; this is what it says when the poller has not been able to (a
    // phone off for a day, a node that never attached), instead of claiming
    // a listing is somewhere it is not.
    // And on a board somebody still reads: past a weekly rollover the notice
    // is on last week's, which the poll moves on its next pass.
    val live = o.optString("board").isNotBlank() && !standStale(o.optString("board")) &&
        System.currentTimeMillis() / 1000 - o.optLong("postedAt") < Listings.TTL_SECONDS
    // Asked for a board and not on one: the poll is trying (Listings.needRefresh).
    val waiting = !live && o.optBoolean("wanted")
    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(
                    // The kind's own icon, like the seeker's card. A house
                    // stood in for "not a vehicle", which put a roof on
                    // everything the moment the board held five nouns.
                    listingIcon(o.optInt("kind")),
                    null, Modifier.size(18.dp),
                )
                Spacer(Modifier.width(8.dp))
                Text(
                    isolate(o.optString("title")),
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 2,
                    overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                )
            }
            Spacer(Modifier.height(4.dp))
            // "each side stakes USD 0.00" is not a fact, it is a leftover.
            //
            // A stake below §17.9's floor is deliberately nothing — holding a
            // fee-sized deposit costs more to hand back than it is worth — so
            // a cheap listing genuinely has none. The card a *seeker* sees has
            // always known that and left the line off. The owner's own row
            // printed the zero, which reads like a broken calculation on the
            // one screen where somebody would notice.
            val deposit = o.optLong("depositPxmr")
            Text(
                if (deposit > 0) {
                    stringResource(
                        R.string.rent_price_and_stake,
                        Amounts.show(context, o.optLong("pricePxmr")).primary,
                        Amounts.show(context, deposit).primary,
                    )
                } else {
                    Amounts.show(context, o.optLong("pricePxmr")).primary
                },
                style = MaterialTheme.typography.bodySmall,
            )
            Text(
                o.optString("area").takeIf { it.isNotBlank() }?.let { isolate(it) }
                    ?: stringResource(R.string.rent_no_area),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            // How many are left, and a way to say one just went.
            //
            // A listing was post-only: an owner who sold one of six had to
            // take the whole thing down and write it again, so the count
            // would have been a number you could set exactly once. The board
            // follows on the next refresh, which is the same path a price
            // change would take.
            if (o.optInt("kind") != Listings.KIND_SKILL) {
                val left = Listings.quantityOf(o)
                Spacer(Modifier.height(8.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        stringResource(R.string.rent_n_left, left),
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Spacer(Modifier.weight(1f))
                    IconButton(
                        onClick = { onQuantity(left - 1) },
                        enabled = left > 1,
                    ) { Icon(Icons.Filled.Remove, stringResource(R.string.rent_one_fewer)) }
                    IconButton(
                        onClick = { onQuantity(left + 1) },
                        enabled = left < Listings.MAX_QUANTITY,
                    ) { Icon(Icons.Filled.Add, stringResource(R.string.rent_one_more)) }
                }
            }
            Spacer(Modifier.height(8.dp))
            Text(
                stringResource(
                    when {
                        live -> R.string.rent_live
                        waiting -> R.string.rent_waiting_board
                        else -> R.string.rent_not_posted
                    },
                ),
                style = MaterialTheme.typography.labelMedium,
                color = if (live) MaterialTheme.colorScheme.primary
                else MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            Row {
                if (live) {
                    OutlinedButton(onClick = onStop) { Text(stringResource(R.string.rent_take_down)) }
                } else {
                    Button(onClick = onPost) { Text(stringResource(R.string.rent_post_it)) }
                }
                Spacer(Modifier.width(8.dp))
                // Deleting a live listing takes it off the board on the way
                // out — one gesture, not a two-step the owner has to know.
                TextButton(onClick = onDelete) { Text(stringResource(R.string.rent_delete)) }
            }
        }
    }
}

/**
 * The form.
 *
 * Split in two on the screen the way §16.18 splits it on the wire: what
 * strangers will see, and what only the person who books it will. The second
 * heading is not decoration — it is the only place a user is told that the
 * first half is public, at the moment they are typing into it.
 */
/** The form on its own, for rendering and review. */
@Composable
fun ListingFormPreview(kind: Int) = ListingForm(kind = kind, onDone = {})

@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
internal fun ListingForm(kind: Int, onDone: () -> Unit) {
    val context = LocalContext.current
    val vehicle = kind == Listings.KIND_VEHICLE
    // The three kinds added in 0.89 carry no typed extras (§16.18): a title,
    // a price, an area, a category and a few tags. Everything below that is
    // about a gearbox or a bedroom is theirs to skip.
    val plain = Listings.isPlain(kind)
    // Every field somebody types is saveable, because the activity is
    // recreated by a rotation, a theme switch at sunset, or a language
    // change — and an eighteen-field listing that empties itself when the
    // phone turns sideways is one nobody fills in twice. The till already
    // does this for a basket; the form it inspired did not inherit it.
    var title by rememberSaveable { mutableStateOf("") }
    var area by rememberSaveable { mutableStateOf("") }
    var price by rememberSaveable { mutableStateOf("") }
    // Vehicle
    var make by rememberSaveable { mutableStateOf("") }
    var model by rememberSaveable { mutableStateOf("") }
    var year by rememberSaveable { mutableStateOf("") }
    var color by rememberSaveable { mutableStateOf("") }
    var seats by rememberSaveable { mutableStateOf("") }
    var trim by rememberSaveable { mutableStateOf("") }
    var gearbox by rememberSaveable { mutableStateOf(2L) }
    var fuel by rememberSaveable { mutableStateOf(1L) }
    // Place
    var rooms by rememberSaveable { mutableStateOf("") }
    var sleeps by rememberSaveable { mutableStateOf("") }
    var sizeM2 by rememberSaveable { mutableStateOf("") }
    var whole by rememberSaveable { mutableStateOf(true) }
    // Plain kinds: a category, and free words for what it actually is.
    var category by rememberSaveable { mutableStateOf(1) }
    // How many of it. Starts at one, because almost every listing is one
    // thing and the number should be answered before it is asked. Cleared to
    // nothing it still means one — an empty box is not a listing of nothing.
    var howMany by rememberSaveable { mutableStateOf("1") }
    var tags by rememberSaveable { mutableStateOf("") }
    // Private
    var details by rememberSaveable { mutableStateOf("") }
    var fix by remember { mutableStateOf<Pair<Long, Long>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }

    // Back closes the form — but not over the top of a half-written listing.
    //
    // Without a handler at all it went past the mode shell to the activity and
    // quit the app; with `onDone` it stopped quitting and went on discarding,
    // which on this screen is nearly the same loss. There is no draft anywhere:
    // `rememberSaveable` carries these fields through a rotation, not through a
    // leaving, and eighteen of them is twenty minutes of somebody's evening. An
    // edge swipe while scrolling the private-details box at the bottom is all it
    // takes, and unlike the Cancel button beside Post it is not a thing anybody
    // aimed at. So it asks, the same courtesy a bar tab gets before it is
    // deleted, and only when there is something to lose — an empty form
    // discarded is just leaving.
    var confirmDiscard by remember { mutableStateOf(false) }
    // Putting a listing up is not instant and cannot be given a bar.
    //
    // §16.18.1's stamp is a search with a geometric distribution: eight bits
    // means 256 evaluations *on average*, and a meaningful share of posts take
    // three or four times that — a few seconds usually, ten or more on an
    // unlucky phone — before the ladder walk even starts. There is no honest
    // percentage to show, because the next attempt is as likely to be the last
    // one as the first was. So it is an indeterminate wait with the button
    // held, which is the difference between "working" and "nothing happened".
    var posting by remember { mutableStateOf(false) }
    // One listing per form, however many times Post is pressed: a retry after
    // a failed board write must re-post the same draft, not save a second copy
    // that the poller then puts up beside the first.
    var draftId by rememberSaveable { mutableStateOf<String?>(null) }
    // The post in flight, if any, so the screen that comes back after a
    // rotation collects what the one that left started. The post itself
    // runs in ListingPosts and finishes either way; what died with the
    // screen was the answer — `posting` came back false over a walk still
    // going, and a second press met a record mid-tenancy (Listings.putDraft
    // for what that cost).
    var postingId by rememberSaveable { mutableStateOf<String?>(null) }
    LaunchedEffect(postingId) {
        val id = postingId ?: return@LaunchedEffect
        posting = true
        val r = ListingPosts.await(id)
        when {
            // Nothing under the id: the process died with the post in it.
            // The record says what happened; the list shows it.
            r == null -> {}
            // A full board answers false rather than throwing, and the
            // screen closed on it as though the thing were up.
            r.isSuccess -> if (!r.getOrThrow()) error = context.getString(R.string.rent_board_full)
            else -> {
                DucatLog.w("Renting", "post: ${r.exceptionOrNull()?.message}")
                // The listing is saved either way and the poller will try
                // again — but a person who cannot post because no node is
                // reachable should be told that, not left looking at a
                // screen that closed.
                error = moneyFailure(context, r.exceptionOrNull()!!, R.string.rent_post_failed)
            }
        }
        posting = false
        ListingPosts.forget(id)
        // Last: this is the effect's own key, and clearing it restarts it.
        postingId = null
        if (error == null) onDone()
    }
    val started = listOf(
        title, area, price, make, model, year, color, seats, trim,
        rooms, sleeps, sizeM2, tags, details,
    ).any { it.isNotBlank() }
    BackHandler { if (started) confirmDiscard = true else onDone() }
    if (confirmDiscard) {
        AlertDialog(
            onDismissRequest = { confirmDiscard = false },
            title = { Text(stringResource(R.string.rent_discard_title)) },
            text = { Text(stringResource(R.string.rent_discard_body)) },
            confirmButton = {
                TextButton(onClick = { confirmDiscard = false; onDone() }) {
                    Text(
                        stringResource(R.string.rent_discard_confirm),
                        color = MaterialTheme.colorScheme.error,
                    )
                }
            },
            dismissButton = {
                TextButton(onClick = { confirmDiscard = false }) {
                    Text(stringResource(R.string.rent_keep_editing))
                }
            },
        )
    }

    // The Post button is dead without a fix — a listing has to sit on some
    // board, and which board is the question a position answers. So this
    // screen asks for the permission itself. It used to assume the answer was
    // already yes, and somebody who had never hailed a ride could fill in the
    // whole form and find the button greyed out under a sentence about
    // waiting for a fix that was never going to come.
    var asked by remember { mutableStateOf(false) }
    var attempt by remember { mutableIntStateOf(0) }
    val locPerm = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.RequestPermission(),
    ) { granted ->
        asked = true
        if (granted) attempt++
    }
    LaunchedEffect(attempt) {
        if (!locationAllowed(context)) {
            locPerm.launch(android.Manifest.permission.ACCESS_FINE_LOCATION)
            return@LaunchedEffect
        }
        grabFix(context) { fix = it }
    }

    // Priced in the owner's own money by default. Nobody knows what 0.0034 XMR
    // is; everybody knows what a night in their spare room is worth. What was
    // typed travels with the listing so [Listings.reprice] can hold it there
    // as the rate moves — the wire only carries piconero, and a standing price
    // that quietly drifts is a price the owner never set.
    val rateVersion by org.ducatproject.ducat.ContactStore.changes.collectAsState()
    val rate = remember(rateVersion) {
        org.ducatproject.ducat.RateStore(context).cached()?.first
    }
    val cur = remember(rateVersion) { Amounts.currency(context) }
    var fiat by rememberSaveable { mutableStateOf(Amounts.enterFiat(context)) }

    // BigDecimal, like every other price in the app: a Double loses the
    // last piconero of a long figure and wraps silently on a huge one.
    val pricePxmr = remember(price, fiat, rate) {
        val v = Amounts.parse(price) ?: return@remember 0L
        val xmr = if (fiat) {
            if (rate == null || rate <= 0) return@remember 0L
            v.divide(java.math.BigDecimal.valueOf(rate), 12, java.math.RoundingMode.DOWN)
        } else {
            v
        }
        Amounts.toPxmr(xmr) ?: 0L
    }
    val stake = Stakes.stakeFor(Listings.dealFor(kind), pricePxmr)

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp)) {
        Text(
            stringResource(listingFormTitle(kind)),
            style = MaterialTheme.typography.titleLarge,
        )
        Spacer(Modifier.height(4.dp))
        Text(
            stringResource(R.string.rent_public_note),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(12.dp))
        OutlinedTextField(
            value = title, onValueChange = { title = it.take(60) },
            label = { Text(stringResource(R.string.rent_title)) },
            singleLine = true, modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(8.dp))
        OutlinedTextField(
            value = area, onValueChange = { area = it.take(40) },
            label = { Text(stringResource(R.string.rent_area)) },
            supportingText = { Text(stringResource(R.string.rent_area_hint)) },
            singleLine = true, modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(8.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(
                value = price,
                onValueChange = { price = it.filter { c -> Amounts.isNumberChar(c) } },
                label = { Text(stringResource(priceLabel(kind), if (fiat) cur else "XMR")) },
                // What it comes to in monero, under the field, because that is
                // what actually goes on the board and the owner should be able
                // to see it without doing the sum.
                supportingText = if (fiat && pricePxmr > 0) {
                    { Text("${org.ducatproject.ducat.formatXmr(pricePxmr)} XMR") }
                } else {
                    null
                },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                singleLine = true, modifier = Modifier.weight(1f),
            )
            if (rate != null) {
                TextButton(
                    onClick = {
                        val p = pricePxmr
                        fiat = !fiat
                        price = if (p > 0) pxmrToField(p, fiat, rate) else ""
                    },
                    contentPadding = PaddingValues(horizontal = 6.dp),
                ) {
                    Text(
                        stringResource(R.string.rent_in_unit, if (fiat) "XMR" else cur),
                        style = MaterialTheme.typography.labelMedium,
                    )
                }
            }
        }
        if (stake > 0) {
            Spacer(Modifier.height(4.dp))
            Text(
                stringResource(
                    R.string.rent_stake_note,
                    Amounts.show(context, stake).primary,
                ),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        // Not on a skill: the price there is an hour of one person's time,
        // and the wire refuses a count on it for the same reason.
        if (kind != org.ducatproject.ducat.Listings.KIND_SKILL) {
            Spacer(Modifier.height(8.dp))
            OutlinedTextField(
                value = howMany,
                onValueChange = { v ->
                    howMany = v.filter { it.isDigit() }.take(3)
                },
                label = { Text(stringResource(R.string.rent_how_many)) },
                supportingText = { Text(stringResource(R.string.rent_how_many_hint)) },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                singleLine = true, modifier = Modifier.fillMaxWidth(),
            )
        }

        Spacer(Modifier.height(16.dp))
        if (plain) {
            Text(
                stringResource(R.string.rent_category),
                style = MaterialTheme.typography.titleSmall,
            )
            Spacer(Modifier.height(8.dp))
            FlowRow(
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
                (1..Listings.subtypeTop(kind)).forEach { n ->
                    FilterChip(
                        selected = category == n,
                        onClick = { category = n },
                        label = { Text(stringResource(categoryLabel(kind, n))) },
                    )
                }
            }
            Spacer(Modifier.height(12.dp))
            val posting = boardTags(tags)
            OutlinedTextField(
                value = tags, onValueChange = { tags = it.take(120) },
                label = { Text(stringResource(R.string.rent_tags)) },
                supportingText = {
                    // What the board will carry, but only once that is not
                    // what was typed — a tag cut to sixteen characters or a
                    // ninth one dropped. Silence there was the whole bug;
                    // saying it when nothing was lost is just noise, and the
                    // comparison is against the same tags unlimited rather
                    // than against the raw text, or re-spacing a list would
                    // trip it.
                    val asTyped = tags.split(',').map { it.trim() }.filter { it.isNotEmpty() }
                    if (posting != asTyped) {
                        Text(stringResource(R.string.rent_tags_on_board, posting.joinToString(", ")))
                    } else {
                        Text(stringResource(R.string.rent_tags_hint))
                    }
                },
                singleLine = true, modifier = Modifier.fillMaxWidth(),
            )
        }
        if (vehicle) {
            Row {
                OutlinedTextField(
                    value = make, onValueChange = { make = it.take(24) },
                    label = { Text(stringResource(R.string.rent_make)) },
                    singleLine = true, modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.width(8.dp))
                OutlinedTextField(
                    value = model, onValueChange = { model = it.take(24) },
                    label = { Text(stringResource(R.string.rent_model)) },
                    singleLine = true, modifier = Modifier.weight(1f),
                )
            }
            Spacer(Modifier.height(8.dp))
            Row {
                OutlinedTextField(
                    value = year, onValueChange = { year = Amounts.typedNumber(it).filter { c -> c in '0'..'9' }.take(4) },
                    label = { Text(stringResource(R.string.rent_year)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    singleLine = true, modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.width(8.dp))
                OutlinedTextField(
                    value = color, onValueChange = { color = it.take(24) },
                    label = { Text(stringResource(R.string.rent_color)) },
                    singleLine = true, modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.width(8.dp))
                OutlinedTextField(
                    value = seats, onValueChange = { seats = Amounts.typedNumber(it).filter { c -> c in '0'..'9' }.take(2) },
                    label = { Text(stringResource(R.string.rent_seats)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    singleLine = true, modifier = Modifier.weight(1f),
                )
            }
            Spacer(Modifier.height(8.dp))
            OutlinedTextField(
                value = trim, onValueChange = { trim = it.take(24) },
                label = { Text(stringResource(R.string.rent_trim)) },
                supportingText = { Text(stringResource(R.string.rent_trim_hint)) },
                singleLine = true, modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(8.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                FilterChip(
                    selected = gearbox == 1L, onClick = { gearbox = 1L },
                    label = { Text(stringResource(R.string.rent_manual)) },
                )
                Spacer(Modifier.width(8.dp))
                FilterChip(
                    selected = gearbox == 2L, onClick = { gearbox = 2L },
                    label = { Text(stringResource(R.string.rent_automatic)) },
                )
            }
            Spacer(Modifier.height(8.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                listOf(
                    1L to R.string.rent_petrol, 2L to R.string.rent_diesel,
                    3L to R.string.rent_electric, 4L to R.string.rent_hybrid,
                ).forEach { (v, label) ->
                    FilterChip(
                        selected = fuel == v, onClick = { fuel = v },
                        label = { Text(stringResource(label)) },
                    )
                    Spacer(Modifier.width(6.dp))
                }
            }
        } else if (!plain) {
            // A place's own fields, and only a place's. This was a plain
            // `else`, which was right while a board held two nouns and put
            // bedrooms and "whole place / private room" on the form for an
            // electrician the moment it held five. The posting path had
            // already been taught the difference; the screen had not.
            Row {
                OutlinedTextField(
                    value = rooms, onValueChange = { rooms = Amounts.typedNumber(it).filter { c -> c in '0'..'9' }.take(2) },
                    label = { Text(stringResource(R.string.rent_rooms)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    singleLine = true, modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.width(8.dp))
                OutlinedTextField(
                    value = sleeps, onValueChange = { sleeps = Amounts.typedNumber(it).filter { c -> c in '0'..'9' }.take(2) },
                    label = { Text(stringResource(R.string.rent_sleeps)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    singleLine = true, modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.width(8.dp))
                OutlinedTextField(
                    value = sizeM2, onValueChange = { sizeM2 = Amounts.typedNumber(it).filter { c -> c in '0'..'9' }.take(4) },
                    label = { Text(stringResource(R.string.rent_size)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    singleLine = true, modifier = Modifier.weight(1f),
                )
            }
            Spacer(Modifier.height(8.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                FilterChip(
                    selected = whole, onClick = { whole = true },
                    label = { Text(stringResource(R.string.rent_whole_place)) },
                )
                Spacer(Modifier.width(8.dp))
                FilterChip(
                    selected = !whole, onClick = { whole = false },
                    label = { Text(stringResource(R.string.rent_private_room)) },
                )
            }
        }

        Spacer(Modifier.height(24.dp))
        HorizontalDivider()
        Spacer(Modifier.height(12.dp))
        Text(
            stringResource(R.string.rent_private_heading),
            style = MaterialTheme.typography.titleSmall,
        )
        Spacer(Modifier.height(4.dp))
        Text(
            stringResource(R.string.rent_private_note),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(8.dp))
        OutlinedTextField(
            value = details, onValueChange = { details = it.take(600) },
            label = { Text(stringResource(R.string.rent_private_label)) },
            minLines = 3, modifier = Modifier.fillMaxWidth(),
        )

        error?.let {
            Spacer(Modifier.height(8.dp))
            Text(it, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall)
        }
        Spacer(Modifier.height(16.dp))
        Row {
            Button(
                enabled = !posting && title.isNotBlank() && pricePxmr > 0 && fix != null,
                onClick = {
                    val here = fix ?: return@Button
                    val specs = JSONObject().apply {
                        if (vehicle) {
                            if (make.isNotBlank()) put("make", make)
                            if (model.isNotBlank()) put("model", model)
                            year.toLongOrNull()?.let { put("year", it) }
                            if (color.isNotBlank()) put("color", color)
                            if (trim.isNotBlank()) put("trim", trim)
                            seats.toLongOrNull()?.let { put("seats", it) }
                            put("gearbox", gearbox)
                            put("fuel", fuel)
                            put("subtype", 1L)
                        } else if (plain) {
                            put("subtype", category.toLong())
                        } else {
                            rooms.toLongOrNull()?.let { put("rooms", it) }
                            sleeps.toLongOrNull()?.let { put("sleeps", it) }
                            sizeM2.toLongOrNull()?.let { put("size_m2", it) }
                            put("subtype", if (whole) 1L else 2L)
                        }
                        // Eight short tags at most, and the wire refuses more
                        // — a summary, because the description belongs in the
                        // conversation where it is not being broadcast.
                        put(
                            "features",
                            JSONArray().also { a ->
                                boardTags(tags).forEach { t -> a.put(t) }
                            },
                        )
                    }
                    val draft = Listings.draft(
                        context, kind, title, area, pricePxmr,
                        here.first, here.second, specs, details,
                        priceTyped = price.takeIf { fiat },
                        priceCurrency = cur.takeIf { fiat },
                        quantity = howMany.toLongOrNull() ?: 1L,
                    )
                    draftId?.let { draft.put("id", it) } ?: run { draftId = draft.optString("id") }
                    error = null
                    posting = true
                    val id = draft.optString("id")
                    ListingPosts.start(id, context) { app ->
                        // Over the record, not instead of it: a retry keeps
                        // the tenancy the first attempt may have taken.
                        Listings.putDraft(app, draft)
                        Listings.post(app, id)
                    }
                    postingId = id
                },
                modifier = Modifier.weight(1f).height(48.dp),
            ) {
                if (posting) CircularProgressIndicator(
                    Modifier.size(18.dp), strokeWidth = 2.dp,
                ) else Text(stringResource(R.string.rent_post_it))
            }
            Spacer(Modifier.width(8.dp))
            TextButton(onClick = onDone) { Text(stringResource(R.string.rent_cancel)) }
        }
        if (posting) {
            // A board write is a DHT publish and an empty cell takes its
            // twenty-one seconds — long enough that a button spinner alone
            // reads as a hang. The comet plus the sentence say what is
            // actually happening and that leaving it alone is fine.
            Spacer(Modifier.height(8.dp))
            DucatBar(progress = null)
            Spacer(Modifier.height(6.dp))
            Text(
                stringResource(R.string.rent_posting_board),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (fix == null) {
            val allowed = locationAllowed(context)
            Spacer(Modifier.height(6.dp))
            Text(
                stringResource(
                    if (allowed) R.string.rent_search_no_fix else R.string.rent_need_location,
                ),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            OutlinedButton(
                onClick = {
                    if (allowed) attempt++
                    else askForLocation(context, asked) { locPerm.launch(it) }
                },
            ) {
                Text(
                    stringResource(
                        if (allowed) R.string.rent_search_retry else R.string.rent_search_allow,
                    ),
                )
            }
        }
        Spacer(Modifier.height(24.dp))
    }
}

/**
 * Posts in flight, outliving the screen that started them.
 *
 * A board post is seconds of Argon2 and a ladder walk that can read sixteen
 * boards, and the form ran it in `rememberCoroutineScope` — cancelled by a
 * rotation, while the blocking post inside ran on regardless. Keyed by the
 * listing's id, which the form saves, so the instance that comes back finds
 * the job the one that left started and takes its result. Same shape as
 * Pay's sends, for the same reason.
 */
private object ListingPosts {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val jobs = java.util.concurrent.ConcurrentHashMap<String, Deferred<Result<Boolean>>>()

    /** Start one if it is not already running; the body gets the application. */
    fun start(id: String, context: android.content.Context, block: (android.content.Context) -> Boolean) {
        val app = context.applicationContext
        jobs.getOrPut(id) { scope.async { runCatching { block(app) } } }
    }

    /** Null when nothing runs under that id: the process died with it. */
    suspend fun await(id: String): Result<Boolean>? = jobs[id]?.await()

    fun forget(id: String) { jobs.remove(id) }
}
