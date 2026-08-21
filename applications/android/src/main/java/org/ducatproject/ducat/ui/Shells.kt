package org.ducatproject.ducat.ui

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
                ShellTab(stringResource(R.string.shells_tab_listings), Icons.Filled.House) {
                    // Everything but the sale: that one has its own mode now,
                    // and a listing shown in both would be managed in both.
                    RentingScreen(kinds = Listings.KINDS - Listings.KIND_SALE)
                },
                ShellTab(stringResource(R.string.shells_tab_bookings), Icons.Filled.Receipt) {
                    BookingsList()
                },
            ),
        )
        // Browse first, because most of the time this mode is opened to look
        // rather than to list — and the tile that opens it is the one on the
        // home screen labelled Marketplace, which is a shopper's word.
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
                    RentingScreen(kinds = listOf(Listings.KIND_SALE))
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
private fun BookingsList() {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val rows = remember(version) {
        org.ducatproject.ducat.Ceremony.all(context)
            .filter {
                it.optInt("kind") == org.ducatproject.ducat.Ceremony.KIND_RESERVATION &&
                    !org.ducatproject.ducat.Ceremony.isArbiter(it)
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
                        // Who it is with — unless that is already the title.
                        //
                        // The condition used to be "there is an `about`",
                        // which stopped tracking what the title actually said
                        // the moment the title was allowed to decline the
                        // `about` and fall back to the person. That row read
                        // "Unnamed contact / USD 0.12 / Finished · Unnamed
                        // contact": the same three words twice, once as the
                        // subject and once as the company.
                        peer?.displayName()
                            ?.takeIf { it != title }
                            ?.let { "${stringResource(state)} · $it" }
                            ?: stringResource(state),
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
        Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp)) {
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
