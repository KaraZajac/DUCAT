package org.ducatproject.ducat.ui

import androidx.activity.compose.BackHandler
import androidx.annotation.StringRes
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.DirectionsCar
import androidx.compose.material.icons.filled.House
import androidx.compose.material.icons.filled.Inventory2
import androidx.compose.material.icons.filled.LocalBar
import androidx.compose.material.icons.filled.Build
import androidx.compose.material.icons.filled.LocalOffer
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.PointOfSale
import androidx.compose.material.icons.filled.Receipt
import androidx.compose.material.icons.filled.RadioButtonUnchecked
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Speed
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.Listings
import org.ducatproject.ducat.MainActivity
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mode
import org.ducatproject.ducat.R
import org.ducatproject.ducat.RunningTab
import org.ducatproject.ducat.TabStore
import org.ducatproject.ducat.formatXmr

/**
 * A mode is a different app (§15.11), not a feature of this one.
 *
 * The person holding a till thinks in sales; the person driving thinks in
 * fares and a meter. Handing them the wallet's five tabs with one swapped
 * screen made the job share a bar with Chat and Activity it never uses. A
 * shell owns the whole scaffold — its own name in the top bar, its own bottom
 * tabs, nothing else — and the only way out is the hamburger and *Personal*,
 * which is the point: switching a job off should feel like putting down the
 * till, not toggling a setting.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ModeShell(mode: Mode, openDrawer: () -> Unit) {
    when (mode) {
        Mode.Pos -> Shell(
            title = stringResource(R.string.shells_title_pos),
            openDrawer = openDrawer,
            tabs = listOf(
                ShellTab(stringResource(R.string.shells_tab_register), Icons.Filled.PointOfSale) { PosScreen() },
                ShellTab(stringResource(R.string.shells_tab_sales), Icons.Filled.Receipt) {
                    SettledList(origin = "pos", emptyTextRes = R.string.shells_no_sales_yet)
                },
                ShellTab(stringResource(R.string.items_tab), Icons.Filled.Inventory2) { ItemsScreen() },
            ),
        )
        Mode.BarTab -> Shell(
            title = stringResource(R.string.shells_title_bar_tab),
            openDrawer = openDrawer,
            tabs = listOf(
                ShellTab(stringResource(R.string.shells_tab_tabs), Icons.Filled.LocalBar) { BarTabScreen(section = "open") },
                ShellTab(stringResource(R.string.shells_tab_closed), Icons.Filled.Receipt) { BarTabScreen(section = "closed") },
                ShellTab(stringResource(R.string.items_tab), Icons.Filled.Inventory2) { ItemsScreen() },
            ),
        )
        Mode.Taxi -> Shell(
            title = stringResource(R.string.shells_title_taxi),
            openDrawer = openDrawer,
            tabs = listOf(
                ShellTab(stringResource(R.string.shells_tab_fares), Icons.Filled.Search) { DriveScreen() },
                ShellTab(stringResource(R.string.shells_tab_meter), Icons.Filled.Speed) { TaxiScreen() },
                ShellTab(stringResource(R.string.shells_tab_rides), Icons.Filled.DirectionsCar) {
                    SettledList(origin = "taxi", emptyTextRes = R.string.shells_no_rides_yet)
                },
            ),
        )
        Mode.Donate -> Shell(
            title = stringResource(R.string.shells_title_donations),
            openDrawer = openDrawer,
            tabs = listOf(
                ShellTab(stringResource(R.string.shells_tab_code), Icons.Filled.RadioButtonUnchecked) { DonateScreen() },
            ),
        )
        Mode.Renting -> Shell(
            title = stringResource(R.string.shells_title_renting),
            openDrawer = openDrawer,
            tabs = listOf(
                ShellTab(stringResource(R.string.shells_tab_browse), Icons.Filled.Search) {
                    // Which noun to open on comes from the tile that was
                    // tapped — a room, a car and a kayak are three doors into
                    // the same job. `key` rather than a parameter alone: the
                    // chip selection is remembered inside, so landing on a
                    // different noun has to be a fresh composition or the
                    // screen would keep showing the last one.
                    val want by MainActivity.browseKind.collectAsState()
                    androidx.compose.runtime.key(want) {
                        RentBrowse(
                            initial = want ?: Listings.KIND_PLACE,
                            onOpenChat = { MainActivity.openChat.value = it.personaHex },
                        )
                    }
                },
                ShellTab(stringResource(R.string.shells_tab_listings), Icons.Filled.House) {
                    RentingScreen(kinds = Listings.RENT_KINDS)
                },
                ShellTab(stringResource(R.string.shells_tab_bookings), Icons.Filled.Receipt) {
                    BookingsList(kinds = Listings.RENT_KINDS)
                },
            ),
        )
        // Three jobs, one shape.
        //
        // Browse first in each, because these are opened to look far more
        // often than to list; then what you are offering; then what has been
        // agreed. The only difference between them is which nouns they are
        // about — renting keeps its chips because three home tiles lead into
        // it, and the other two have one noun and nothing to switch to.
        Mode.Marketplace -> Shell(
            title = stringResource(R.string.mode_marketplace),
            openDrawer = openDrawer,
            tabs = listOf(
                ShellTab(stringResource(R.string.shells_tab_browse), Icons.Filled.Search) {
                    // The same route the bookings list takes to a thread: a
                    // shell has no chat of its own to open one in, so it asks
                    // the activity, which owns the overlay.
                    MarketBrowse(onOpenChat = { MainActivity.openChat.value = it.personaHex })
                },
                ShellTab(
                    stringResource(R.string.shells_tab_my_listings),
                    Icons.Filled.LocalOffer,
                ) {
                    RentingScreen(kinds = Listings.SALE_KINDS)
                },
                // Not "Bookings": nothing is booked, something is bought or
                // sold, and it holds both directions.
                ShellTab(stringResource(R.string.shells_tab_deals), Icons.Filled.Receipt) {
                    BookingsList(kinds = Listings.SALE_KINDS)
                },
            ),
        )
        Mode.HireHelp -> Shell(
            title = stringResource(R.string.mode_hire_help),
            openDrawer = openDrawer,
            tabs = listOf(
                ShellTab(stringResource(R.string.shells_tab_browse), Icons.Filled.Search) {
                    HireBrowse(onOpenChat = { MainActivity.openChat.value = it.personaHex })
                },
                ShellTab(
                    stringResource(R.string.shells_tab_my_skills),
                    Icons.Filled.Build,
                ) {
                    RentingScreen(kinds = Listings.SKILL_KINDS)
                },
                ShellTab(stringResource(R.string.shells_tab_jobs), Icons.Filled.Receipt) {
                    BookingsList(kinds = Listings.SKILL_KINDS)
                },
            ),
        )
        // Deliberately not a Shell: a shell has a hamburger, and a hamburger
        // is a way for a customer to walk into somebody's wallet. The kiosk
        // draws its own bar and its only exit is the PIN.
        Mode.Kiosk -> KioskScreen()
        Mode.None -> {} // personal mode renders the full app, not a shell
    }
}

/**
 * Which side of this deal the reader is on.
 *
 * The list holds both — an escrow is one agreement between two people, and
 * filtering it to the half where you are the seller would hide the half where
 * you are the one who has to pay or sign. Which made every row ambiguous:
 * "Sam · USD 0.12 · Finished" reads exactly the same whether you bought that
 * coffee grinder or sold it.
 *
 * The funder is whoever pays the price, so they are the buyer, the guest, the
 * client. Worded per job rather than once, because "renting" said of both
 * sides of a rental is no help at all.
 */
private fun sideLabel(aboutKind: Int, funder: Boolean): Int = when (aboutKind) {
    Listings.KIND_SALE ->
        if (funder) R.string.book_side_buying else R.string.book_side_selling
    Listings.KIND_SKILL ->
        if (funder) R.string.book_side_hiring else R.string.book_side_working
    // A place, a vehicle, gear — and anything struck before escrows recorded
    // what they were about, which lands here for the same reason it lands in
    // Renting's tab.
    else ->
        if (funder) R.string.book_side_renting else R.string.book_side_letting
}

/**
 * What has been booked, and where each one has got to.
 *
 * This tab used to read the till's ledger filtered to `origin = "rent"` —
 * entries nothing has ever written, because a booking is not a bill somebody
 * rang up. It was a promise the app could not keep: an owner could take a
 * booking a day and the tab would say "No bookings yet." forever.
 *
 * A booking *is* a reservation escrow (§16.18), so that is what it lists,
 * newest first, with the listing it came from when the thread remembers one.
 */
@Composable
private fun BookingsList(kinds: List<Int> = Listings.KINDS) {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val rows = remember(version, kinds) {
        org.ducatproject.ducat.Ceremony.all(context)
            .filter {
                it.optInt("kind") == org.ducatproject.ducat.Ceremony.KIND_RESERVATION &&
                    !org.ducatproject.ducat.Ceremony.isArbiter(it)
            }
            // To the mode whose job it was.
            //
            // `aboutKind` is the listing kind snapshotted when the escrow was
            // struck — not the ceremony kind, which only says "reservation".
            // Escrows older than that snapshot have none, and rather than
            // vanish from all three modes or appear in all three they stay
            // where they have always been: Renting, which is the only mode
            // that had a bookings tab when they were made.
            .filter { o ->
                val about = o.optInt("aboutKind", 0)
                if (about == 0) Listings.KIND_PLACE in kinds else about in kinds
            }
            .sortedByDescending { it.optLong("created") }
    }
    if (rows.isEmpty()) {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Text(
                stringResource(R.string.shells_no_bookings_yet),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        return
    }
    val contacts = remember(version) { ContactStore(context).all() }
    // Which of these are waiting on the reader rather than on the far side.
    //
    // "Waiting to be funded" is true of both and useful to neither: it is the
    // same six words whether the host has yet to accept or the guest has yet
    // to pay, so the one person who can move the deal along cannot tell from
    // this list that it is them. The home screen says so now, and a host lives
    // in Renting rather than on the home screen.
    val mine = remember(version) {
        org.ducatproject.ducat.Ceremony.waitingOnMe(context)
            .mapNotNull { it.optString("id").takeIf { s -> s.isNotEmpty() } }
            .toSet()
    }
    // Which rows may guess their subject from the thread.
    //
    // A booking struck before escrows carried their own label has to fall back
    // to what the conversation is about — but the conversation moves on, so on
    // the second deal with somebody the first one relabels itself with the
    // second one's title. Two rows, different amounts, same name, one of them
    // a lie. Rows are newest-first, so only the newest per person may guess:
    // for that one the thread's subject is almost certainly still right, and
    // for the older ones the honest answer is who it was with.
    val mayGuess = remember(rows) {
        val seen = HashSet<String>()
        rows.map { o ->
            org.ducatproject.ducat.Ceremony.otherPrincipal(o)?.let { seen.add(it) } ?: true
        }
    }
    LazyColumn(Modifier.fillMaxSize().padding(horizontal = 16.dp)) {
        itemsIndexed(rows) { index, o ->
            val peerHex = org.ducatproject.ducat.Ceremony.otherPrincipal(o)
            val peer = contacts.firstOrNull { it.personaHex == peerHex }
            val about = peerHex?.let { org.ducatproject.ducat.Enquiries.about(context, it) }
            val need = org.ducatproject.ducat.Ceremony.expectedTotalPxmr(o)
            // Whose turn first, stage second. A settlement parked on this
            // device is `release_pending`, which the stage arm below calls
            // "Settling" — accurate, and indistinguishable from the same word
            // on the row where the *other* side is the one who has to sign.
            val state = if (o.optString("id") in mine) {
                R.string.shells_booking_your_turn
            } else when (o.optString("stage")) {
                "released", "release_cosigned" -> R.string.shells_booking_done
                "releasing", "release_pending" -> R.string.shells_booking_settling
                "aborted" -> R.string.shells_booking_aborted
                else ->
                    if (o.optLong("fundedPxmr") >= need && need > 0) {
                        R.string.shells_booking_secured
                    } else {
                        R.string.shells_booking_waiting
                    }
            }
            Card(
                Modifier.fillMaxWidth().padding(vertical = 6.dp).clickable {
                    peerHex?.let { MainActivity.openChat.value = it }
                },
            ) {
                Column(Modifier.padding(14.dp)) {
                    // The escrow's own snapshot first: the thread's subject
                    // follows the latest conversation, and a booking settled
                    // last month should still say what it was for. `about`
                    // remains the fallback for deals struck before the
                    // snapshot existed.
                    val title = o.optString("aboutTitle").takeIf { it.isNotBlank() }
                        ?: about?.title?.takeIf { mayGuess.getOrElse(index) { false } }
                        ?: peer?.displayName()
                        ?: stringResource(R.string.shells_booking_someone)
                    Text(title, style = MaterialTheme.typography.titleSmall)
                    Spacer(Modifier.height(2.dp))
                    Text(
                        // The price, not the pot. `need` is what the escrow
                        // has to hold — the price plus *both* deposits — which
                        // is the right number for deciding whether the thing
                        // is funded and the wrong one to head a row with: a
                        // fifty-dollar room read seventy-four, a figure
                        // matching nothing either side agreed, receives, or
                        // pays. The deposits come home; the price does not.
                        Amounts.show(context, o.optLong("farePxmr")).primary,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    Spacer(Modifier.height(2.dp))
                    Text(
                        // Which side, then where it has got to, then who with.
                        // Who it is with — unless that is already the title.
                        //
                        // The condition used to be "there is an `about`",
                        // which stopped tracking what the title actually said
                        // the moment the title was allowed to decline the
                        // `about` and fall back to the person. That row read
                        // "Unnamed contact / USD 0.12 / Finished · Unnamed
                        // contact": the same three words twice, once as the
                        // subject and once as the company.
                        run {
                            val side = stringResource(
                                sideLabel(
                                    o.optInt("aboutKind", 0),
                                    org.ducatproject.ducat.Ceremony.isFunder(o),
                                ),
                            )
                            val rest = peer?.displayName()
                                ?.takeIf { it != title }
                                ?.let { "${stringResource(state)} · $it" }
                                ?: stringResource(state)
                            "$side · $rest"
                        },
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

private data class ShellTab(
    val label: String,
    val icon: androidx.compose.ui.graphics.vector.ImageVector,
    val content: @Composable () -> Unit,
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun Shell(title: String, openDrawer: () -> Unit, tabs: List<ShellTab>) {
    var current by remember { mutableStateOf(0) }
    // Back walks the tabs home before it leaves.
    //
    // The activity's own tab handler is gated on `Mode.None` so that it does
    // not swallow presses meant for a shell — and no shell was catching them,
    // so Back from the till's Items tab, or the taxi's Meter, went straight
    // past everything here to the activity and closed the app. A vendor
    // checking a price mid-sale lost the sale.
    //
    // The same rule the personal tabs follow: while there is a tab to come
    // back to, come back to it; on the first tab do nothing at all, because
    // what is outside a job — the drawer, and then putting the phone down —
    // is the honest next step and swallowing Back forever is not.
    BackHandler(enabled = current != 0) { current = 0 }
    Scaffold(
        topBar = {
            TopAppBar(
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.background,
                ),
                title = { Text(title, style = MaterialTheme.typography.titleLarge) },
                navigationIcon = {
                    IconButton(onClick = openDrawer) {
                        Icon(Icons.Filled.Menu,
                            contentDescription = stringResource(R.string.shells_menu))
                    }
                },
            )
        },
        bottomBar = {
            // One tab is no tab: the donate shell is a single standing screen
            // and a bar under it would be a bar with one button.
            if (tabs.size > 1) {
                NavigationBar(
                    containerColor = MaterialTheme.colorScheme.background,
                    tonalElevation = 0.dp,
                ) {
                    tabs.forEachIndexed { i, t ->
                        NavigationBarItem(
                            selected = current == i,
                            onClick = { current = i },
                            icon = { Icon(t.icon, contentDescription = t.label) },
                            label = { Text(t.label) },
                        )
                    }
                }
            }
        },
    ) { padding ->
        Box(Modifier.padding(padding).fillMaxSize()) {
            tabs[current].content()
        }
    }
}

/**
 * The day's ledger for one job: what settled, what's still owed, and the sum
 * that matters at closing time. Shared by the till's Sales tab and the taxi's
 * Rides tab, because both are the same question about a different `origin`.
 */
@Composable
private fun SettledList(origin: String, @StringRes emptyTextRes: Int) {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val store = remember { TabStore(context) }
    val mine = remember(version) { store.all().filter { it.origin == origin } }
    val waiting = mine.filter { it.state == "settled" }
    val done = mine.filter { it.state == "paid" || it.state == "paid_oob" }
    val today = remember { java.util.Calendar.getInstance().apply {
        set(java.util.Calendar.HOUR_OF_DAY, 0); set(java.util.Calendar.MINUTE, 0)
        set(java.util.Calendar.SECOND, 0)
    }.timeInMillis }
    val earnedToday = done.filter { it.settledAt >= today }.sumOf { it.totalPxmr }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        // Sixteen, like every other screen's number — see Pos.kt and Balance.kt.
        Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
            Spacer(Modifier.height(16.dp))
            Text(
                stringResource(R.string.shells_today),
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            val shown = Amounts.show(context, earnedToday)
            Text(shown.primary, style = MaterialTheme.typography.displayLarge)
            shown.secondary?.let {
                Text(it, style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }

        if (waiting.isNotEmpty()) {
            SettledSection(stringResource(R.string.shells_billed_waiting), waiting)
        }
        if (done.isNotEmpty()) {
            SettledSection(stringResource(R.string.shells_settled), done)
        }
        if (mine.isEmpty()) {
            Text(
                stringResource(emptyTextRes),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(24.dp),
            )
        }
        Spacer(Modifier.height(24.dp))
    }
}

@Composable
private fun SettledSection(label: String, tabs: List<RunningTab>) {
    val context = LocalContext.current
    Text(
        label,
        style = MaterialTheme.typography.labelMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(start = 24.dp, top = 20.dp, bottom = 8.dp),
    )
    Card(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
        Column {
            tabs.sortedByDescending { it.settledAt }.forEach { t ->
                val who = remember(t.personaHex) {
                    ContactStore(context).all()
                        .firstOrNull { it.personaHex == t.personaHex }?.displayName()
                        ?: "${t.personaHex.take(8)}…"
                }
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text(
                            who,
                            style = MaterialTheme.typography.bodyLarge,
                            maxLines = 1,
                            overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                        )
                        Text(
                            when (t.state) {
                                "settled" -> if (t.seenTx != null)
                                    stringResource(R.string.shells_payment_seen_settling)
                                    else stringResource(R.string.shells_billed_unpaid)
                                "paid_oob" -> stringResource(R.string.shells_paid_outside)
                                else -> stringResource(R.string.shells_paid)
                            },
                            style = MaterialTheme.typography.labelSmall,
                            color = if (t.state == "settled")
                                MaterialTheme.colorScheme.onSurfaceVariant
                            else MaterialTheme.ducat.settled,
                        )
                    }
                    // Honour the fiat toggle the "Today" total above already
                    // respects, so a vendor who reads in dollars sees each line
                    // in dollars too, not raw XMR.
                    val line = Amounts.show(context, t.totalPxmr)
                    Column(horizontalAlignment = Alignment.End) {
                        Text(
                            line.primary,
                            style = MaterialTheme.typography.bodyMedium,
                            fontFamily = FontFamily.Monospace,
                        )
                        line.secondary?.let {
                            Text(
                                it,
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
            }
        }
    }
}
