package org.ducatproject.ducat.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.BillItem
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.Mode
import org.ducatproject.ducat.ModeStore
import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.Orders
import org.ducatproject.ducat.R
import org.ducatproject.ducat.TabStore
import org.ducatproject.ducat.formatXmr

/**
 * The counter, facing the other way (§15.11's modes, turned outward).
 *
 * Every other screen in this app belongs to the person holding the phone.
 * This one belongs to a stranger standing in front of it: they tap what they
 * want, they tap or scan once to pay, and they never touch anything else.
 *
 * That one gesture is what makes them somebody this shop can talk to, and it
 * is worth their trouble: the bill arrives on their own phone itemised, the
 * payment they make is identified by the transaction they name rather than
 * guessed at, and the receipt lands beside it in their Activity. A bare
 * `monero:` code — which is what this screen used to show, on the argument
 * that a queue for coffee has no time for a handshake — buys a payment and
 * none of the rest. See [org.ducatproject.ducat.Orders].
 *
 * Which makes the way *out* the important part. Left alone, a customer could
 * press Back into somebody's wallet and their chats, so leaving is behind the
 * PIN and there is no other exit: not the drawer, not the tabs, because a
 * kiosk has neither.
 */
@Composable
fun KioskScreen() {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    var basket by remember { mutableStateOf(listOf<BillItem>()) }
    var tipPct by rememberSaveable { mutableStateOf(0) }
    // The order id rather than the order, and saveable rather than
    // remembered: a customer fumbling for their phone while the card is up is
    // exactly when Android decides to rotate the screen or rebuild the
    // activity, and losing the order there dumps them back at an empty basket
    // with a bill already sent. The order itself is re-read from the store, so
    // there is one copy of its state and it is the stored one.
    var placedId by rememberSaveable { mutableStateOf<String?>(null) }
    val placed = remember(placedId, version) {
        placedId?.let { id -> Orders.all(context).firstOrNull { it.id == id } }
    }
    var staffDoor by remember { mutableStateOf(false) }
    var staffOpen by remember { mutableStateOf(false) }

    // The docstring above promises the PIN is the only way out. It was not:
    // with no handler, Back went straight past the mode shell to the activity
    // and closed the app, which leaves whoever pressed it standing in front of
    // an unlocked phone on the launcher — the exact thing the lock is for. A
    // kiosk has nowhere to go back to, so Back shuts whatever is open over the
    // counter and otherwise does nothing at all.
    BackHandler {
        when {
            staffOpen -> staffOpen = false
            staffDoor -> staffDoor = false
            else -> Unit
        }
    }

    // Nudge the store while an order is on screen. Reading it is what draws
    // the panel — `placed` is derived above — so this only has to make sure a
    // payment that lands quietly still produces a bump, at a pace a person
    // standing at a counter reads as immediate.
    LaunchedEffect(placedId) {
        while (placedId != null) {
            kotlinx.coroutines.delay(3_000)
            withContext(Dispatchers.IO) { runCatching { ContactStore.bump() } }
        }
    }

    Scaffold(
        topBar = {
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    stringResource(R.string.kiosk_title),
                    Modifier.weight(1f).padding(start = 8.dp),
                    style = MaterialTheme.typography.titleLarge,
                )
                // The staff door. Visible on purpose: a customer who presses
                // it gets a PIN prompt they cannot pass, and staff should not
                // have to know a secret gesture to serve somebody.
                IconButton(onClick = { staffDoor = true }) {
                    Icon(Icons.Filled.Lock, stringResource(R.string.kiosk_staff))
                }
            }
        },
    ) { padding ->
        Box(Modifier.padding(padding).fillMaxSize()) {
            when {
                staffOpen -> StaffPanel(onClose = { staffOpen = false })
                placed != null -> PayPanel(
                    order = placed,
                    onDone = { placedId = null; basket = emptyList(); tipPct = 0 },
                )
                else -> Ordering(
                    basket = basket,
                    onAdd = { name, pxmr -> basket = basket + BillItem(name, pxmr) },
                    onClear = { basket = emptyList() },
                    // Unpaired: nobody owns this order yet. The panel that
                    // follows shows a card, and whoever claims it gets the
                    // bill — which is the difference between a payment and a
                    // purchase somebody has a record of.
                    tipPct = tipPct,
                    onTip = { tipPct = it },
                    // The tip rides as a line, because core refuses a bill
                    // whose lines do not add up to its total — and because a
                    // customer reading the bill on their own phone should see
                    // what they agreed to, not a number that is larger than
                    // the things they picked.
                    onOrder = {
                        val tip = basket.sumOf { it.amountPxmr } * tipPct / 100
                        val lines =
                            if (tip > 0) {
                                basket + BillItem(context.getString(R.string.kiosk_tip_line), tip)
                            } else {
                                basket
                            }
                        placedId = Orders.begin(context, lines).id
                    },
                )
            }
        }
    }

    PinGate(
        open = staffDoor,
        onDismiss = { staffDoor = false },
        onPassed = { staffDoor = false; staffOpen = true },
        why = R.string.pin_ask_body_staff,
    )
}

/** Tap what you want; see what it comes to. */
@Composable
private fun Ordering(
    basket: List<BillItem>,
    tipPct: Int,
    onAdd: (String, Long) -> Unit,
    onTip: (Int) -> Unit,
    onClear: () -> Unit,
    onOrder: () -> Unit,
) {
    val context = LocalContext.current
    val lines = basket.sumOf { it.amountPxmr }
    val tip = lines * tipPct / 100
    val total = lines + tip
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Spacer(Modifier.height(8.dp))
        ItemPicker(onPick = onAdd)
        Spacer(Modifier.height(16.dp))
        // A kiosk with nothing on its menu draws no buttons at all, and then
        // told the customer to tap one of them. Blank screen, cheerful
        // instruction, nothing to press — and no sign to whoever set it up
        // that the thing is not actually open. Say which of the two it is:
        // never stocked, or everything off for the night.
        val menu = remember(ContactStore.changes.collectAsState().value) {
            org.ducatproject.ducat.Catalogue.live(context) to
                org.ducatproject.ducat.Catalogue.sellable(context)
        }
        if (menu.second.isEmpty()) {
            Text(
                stringResource(
                    if (menu.first.isEmpty()) R.string.kiosk_no_menu
                    else R.string.kiosk_all_off,
                ),
                Modifier.fillMaxWidth().padding(24.dp),
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )
            return@Column
        }
        if (basket.isEmpty()) {
            Text(
                stringResource(R.string.kiosk_empty),
                Modifier.fillMaxWidth().padding(24.dp),
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )
            return@Column
        }
        Card(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
            Column(Modifier.padding(16.dp)) {
                basket.forEach { line ->
                    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
                        Text(line.description, Modifier.weight(1f))
                        Text(Amounts.show(context, line.amountPxmr).primary)
                    }
                }
                if (tip > 0) {
                    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
                        Text(stringResource(R.string.kiosk_tip_line), Modifier.weight(1f))
                        Text(Amounts.show(context, tip).primary)
                    }
                }
                HorizontalDivider(Modifier.padding(vertical = 8.dp))
                Row(Modifier.fillMaxWidth()) {
                    Text(
                        stringResource(R.string.kiosk_total),
                        Modifier.weight(1f),
                        style = MaterialTheme.typography.titleMedium,
                    )
                    Text(
                        Amounts.show(context, total).primary,
                        style = MaterialTheme.typography.titleMedium,
                    )
                }
            }
        }
        Spacer(Modifier.height(12.dp))
        // Percentages, not a keypad. Nobody standing at a counter with a queue
        // behind them types an amount, and a tip nobody leaves is a tip the
        // shop may as well not have offered.
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                stringResource(R.string.kiosk_tip),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            listOf(0, 5, 10, 15).forEach { pct ->
                FilterChip(
                    selected = tipPct == pct,
                    onClick = { onTip(pct) },
                    label = {
                        Text(
                            if (pct == 0) stringResource(R.string.kiosk_tip_none)
                            else "$pct%",
                        )
                    },
                )
            }
        }
        Spacer(Modifier.height(16.dp))
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
            OutlinedButton(onClick = onClear, modifier = Modifier.height(56.dp)) {
                Text(stringResource(R.string.kiosk_start_over))
            }
            Spacer(Modifier.width(12.dp))
            Button(
                onClick = onOrder,
                modifier = Modifier.weight(1f).height(56.dp),
            ) { Text(stringResource(R.string.kiosk_order)) }
        }
        Spacer(Modifier.height(24.dp))
    }
}

/**
 * Tap or scan, then the bill, then the word that the money arrived.
 *
 * The card is the whole point of this screen. A `monero:` code would have
 * been three lines and would have worked — and would have left the customer
 * with a payment nobody itemised, no way to be told the order was ready, and
 * an Activity entry that records money leaving and not what for. The card
 * costs them one gesture and buys all of it.
 */
@Composable
private fun PayPanel(order: Orders.Order, onDone: () -> Unit) {
    val context = LocalContext.current
    // No local copy of the order: the caller re-reads it from the store, so
    // there is one record of where this sale has got to and everybody reads
    // the same one.
    var cardUri by remember(order.id) { mutableStateOf<String?>(null) }
    var cardInbox by remember(order.id) { mutableStateOf<String?>(null) }
    var error by remember(order.id) { mutableStateOf<String?>(null) }
    var fallback by remember(order.id) { mutableStateOf(false) }

    // Issued once per order. A "sale" card never auto-reissues and this flow
    // waits for *its* claimant, so somebody scanning a profile code across the
    // room is not handed the queue's coffee.
    LaunchedEffect(order.id, fallback) {
        if (cardUri != null || fallback || !order.unpaired) return@LaunchedEffect
        val r = withContext(Dispatchers.IO) {
            runCatching {
                Mailbox.issueCard(
                    context, MyProfile(context).name(), CARD_TTL_SECS, purpose = "sale",
                )
            }
        }
        r.onSuccess { cardUri = it.uri; cardInbox = it.inboxKey }
            .onFailure { error = moneyFailure(context, it) }
    }

    // The same card over NFC, for as long as it is on screen: tapping and
    // scanning are the same gesture to a customer and should be the same
    // gesture to us.
    DisposableEffect(cardUri) {
        org.ducatproject.ducat.nfc.Tap.offered = cardUri
        onDispose { org.ducatproject.ducat.nfc.Tap.offered = null }
    }

    // Claim → contact → bill.
    //
    // The loop condition reads the *store*, not the `order` this composable
    // was handed. An effect keyed on cardInbox does not restart when the order
    // changes, so it holds whichever copy it captured — and binding makes a
    // new one. Testing the captured value meant the condition stayed true for
    // ever after a successful bind, and the loop went round again: another
    // tab, another bill, and a customer watching their phone fill up with
    // them. The stored record is the one that knows this sale is done.
    LaunchedEffect(cardInbox) {
        val inbox = cardInbox ?: return@LaunchedEffect
        while (true) {
            val current = withContext(Dispatchers.IO) {
                Orders.all(context).firstOrNull { it.id == order.id }
            } ?: return@LaunchedEffect
            if (!current.unpaired) return@LaunchedEffect
            val who = withContext(Dispatchers.IO) {
                runCatching {
                    Mailbox.collectClaims(context)
                    ContactStore(context).claimantOf(inbox)
                }.getOrNull()
            }
            if (who == null) {
                kotlinx.coroutines.delay(2_000)
                continue
            }
            withContext(Dispatchers.IO) { runCatching { Orders.bind(context, current, who) } }
                .onFailure {
                    // A claim that has landed stays landed, so without a wait
                    // here a node that is down turns this into a spin: bind,
                    // fail, bind again, as fast as the machine allows.
                    // Through the same funnel every other money screen
                    // uses. A bare `it.message` on an unattended shop
                    // display is how "v1=decoys: InterfaceError(…)" gets
                    // shown to a customer; moneyFailure knows the offline
                    // and node cases by name and strips the bridge prefix
                    // off whatever it does not.
                    error = moneyFailure(context, it)
                    kotlinx.coroutines.delay(5_000)
                }
        }
    }

    if (fallback) return MoneroFallback(order, onDone)
    if (order.unpaired) {
        return PairPanel(
            order = order,
            cardUri = cardUri,
            error = error,
            onCancel = onDone,
            onFallback = { fallback = true },
        )
    }
    BilledPanel(order = order, onDone = onDone)
}

/** How long a sale card stays claimable. Two hours outlasts any queue. */
private const val CARD_TTL_SECS: ULong = 7_200uL

/** The card, waiting to be tapped or scanned. */
@Composable
internal fun PairPanel(
    order: Orders.Order,
    cardUri: String?,
    error: String?,
    onCancel: () -> Unit,
    onFallback: () -> Unit,
) {
    val context = LocalContext.current
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(
            Amounts.show(context, order.totalPxmr).primary,
            style = MaterialTheme.typography.headlineMedium,
        )
        Spacer(Modifier.height(12.dp))
        when {
            error != null -> Text(
                stringResource(R.string.kiosk_card_failed, error),
                color = MaterialTheme.colorScheme.error,
                textAlign = TextAlign.Center,
            )
            cardUri == null -> CircularProgressIndicator()
            else -> QrBlock(cardUri)
        }
        Spacer(Modifier.height(12.dp))
        Text(
            stringResource(R.string.kiosk_tap_or_scan),
            style = MaterialTheme.typography.bodyLarge,
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.height(24.dp))
        TextButton(onClick = onFallback) { Text(stringResource(R.string.kiosk_no_ducat)) }
        TextButton(onClick = onCancel) { Text(stringResource(R.string.kiosk_cancel_order)) }
    }
}

/** Billed into their conversation; the rest happens on their phone. */
@Composable
internal fun BilledPanel(order: Orders.Order, onDone: () -> Unit) {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val state = remember(order.id, version) { Orders.stateOf(context, order) }
    if (state != Orders.State.Awaiting) return PaidPanel(order, onDone)
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(
            Icons.Filled.Check, null, Modifier.size(48.dp),
            tint = MaterialTheme.ducat.settled,
        )
        Spacer(Modifier.height(8.dp))
        Text(
            stringResource(R.string.kiosk_bill_sent),
            style = MaterialTheme.typography.headlineSmall,
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.height(4.dp))
        Text(
            stringResource(R.string.kiosk_bill_sent_note),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.height(16.dp))
        Text(
            Amounts.show(context, order.totalPxmr).primary,
            style = MaterialTheme.typography.headlineMedium,
        )
        Spacer(Modifier.height(16.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
            Spacer(Modifier.width(8.dp))
            Text(
                stringResource(R.string.kiosk_waiting),
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        Spacer(Modifier.height(16.dp))
        TextButton(onClick = onDone) { Text(stringResource(R.string.kiosk_next_customer)) }
    }
}

/** Paid, whichever way they paid. */
@Composable
private fun PaidPanel(order: Orders.Order, onDone: () -> Unit) {
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(
            Icons.Filled.Check, null, Modifier.size(64.dp),
            tint = MaterialTheme.ducat.settled,
        )
        Spacer(Modifier.height(8.dp))
        Text(
            stringResource(R.string.kiosk_paid_title),
            style = MaterialTheme.typography.headlineSmall,
        )
        Spacer(Modifier.height(4.dp))
        Text(
            stringResource(R.string.kiosk_paid_number, order.number),
            style = MaterialTheme.typography.displaySmall,
        )
        Spacer(Modifier.height(16.dp))
        Button(
            onClick = onDone,
            modifier = Modifier.fillMaxWidth().height(56.dp),
        ) { Text(stringResource(R.string.kiosk_next_customer)) }
    }
}

/** The old way, for a wallet with no DUCAT behind it. */
@Composable
private fun MoneroFallback(order: Orders.Order, onDone: () -> Unit) {
    val context = LocalContext.current
    // A bare address needs its own order: the noise in the total is how a
    // mempool sighting is told from the next customer's identical coffee, and
    // an unpaired order has none.
    val anon = remember(order.id) { Orders.place(context, order.lines) }
    PayPanelMonero(anon, onDone)
}

/** The code to pay, and then the word that the money arrived. */
@Composable
private fun PayPanelMonero(order: Orders.Order, onDone: () -> Unit) {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val live = remember(order.id, version) {
        Orders.all(context).firstOrNull { it.id == order.id } ?: order
    }
    val paid = live.state != Orders.State.Awaiting
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        if (paid) {
            Icon(
                Icons.Filled.Check, null, Modifier.size(64.dp),
                tint = MaterialTheme.ducat.settled,
            )
            Spacer(Modifier.height(8.dp))
            Text(
                stringResource(R.string.kiosk_paid_title),
                style = MaterialTheme.typography.headlineSmall,
            )
            Spacer(Modifier.height(4.dp))
            Text(
                stringResource(R.string.kiosk_paid_number, live.number),
                style = MaterialTheme.typography.displaySmall,
            )
            Spacer(Modifier.height(16.dp))
            Button(
                onClick = onDone,
                modifier = Modifier.fillMaxWidth().height(56.dp),
            ) { Text(stringResource(R.string.kiosk_next_customer)) }
            return@Column
        }
        Text(
            Amounts.show(context, live.totalPxmr).primary,
            style = MaterialTheme.typography.headlineMedium,
        )
        Text(
            "${formatXmr(live.totalPxmr)} XMR",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(12.dp))
        QrBlock(Orders.payUri(live))
        Spacer(Modifier.height(12.dp))
        Text(
            stringResource(R.string.kiosk_scan_note),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.height(8.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
            Spacer(Modifier.width(8.dp))
            Text(
                stringResource(R.string.kiosk_waiting),
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        Spacer(Modifier.height(16.dp))
        TextButton(onClick = onDone) { Text(stringResource(R.string.kiosk_cancel_order)) }
    }
}

/**
 * What the shop sees once it has proved it is the shop: today's orders, and
 * the way out of kiosk mode.
 */
@Composable
internal fun StaffPanelPreview(tab: Int) = StaffPanel(onClose = {}, startOn = tab)

@Composable
private fun StaffPanel(onClose: () -> Unit, startOn: Int = 0) {
    val context = LocalContext.current
    // The menu, behind the same door as the orders. A stall that has run out
    // of croissants, or wants to put the iced coffee on now that it is warm,
    // should not have to leave kiosk mode, find the menu screen, and set the
    // mode up again — with the customer-facing screen showing somebody's
    // wallet in between. Editing what you sell is shop work, and the PIN has
    // already been answered.
    var tab by rememberSaveable { mutableStateOf(startOn) }
    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(start = 16.dp, top = 8.dp, end = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                stringResource(R.string.kiosk_staff),
                Modifier.weight(1f),
                style = MaterialTheme.typography.titleLarge,
            )
            IconButton(onClick = onClose) {
                Icon(Icons.Filled.Close, stringResource(R.string.kiosk_back_to_counter))
            }
        }
        TabRow(selectedTabIndex = tab) {
            Tab(
                selected = tab == 0, onClick = { tab = 0 },
                text = { Text(stringResource(R.string.kiosk_orders)) },
            )
            Tab(
                selected = tab == 1, onClick = { tab = 1 },
                text = { Text(stringResource(R.string.items_tab)) },
            )
        }
        if (tab == 1) {
            ItemsScreen()
            return@Column
        }
        StaffOrders()
    }
}

/** Today's orders, and the way out of kiosk mode. */
@Composable
private fun StaffOrders() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val version by ContactStore.changes.collectAsState()
    val orders = remember(version) { Orders.all(context) }
    Column(Modifier.fillMaxSize()) {
        Spacer(Modifier.height(8.dp))
        OutlinedButton(
            onClick = { ModeStore(context).set(Mode.None) },
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        ) { Text(stringResource(R.string.kiosk_leave)) }
        Spacer(Modifier.height(8.dp))
        if (orders.isEmpty()) {
            Text(
                stringResource(R.string.kiosk_no_orders),
                Modifier.padding(16.dp),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            return@Column
        }
        LazyColumn(Modifier.fillMaxSize()) {
            items(orders) { o ->
                ListItem(
                    headlineContent = {
                        Text(stringResource(R.string.kiosk_paid_number, o.number))
                    },
                    // Transparent, as everywhere else: a ListItem defaults to
                    // `surface`, which is not the page it sits on.
                    colors = ListItemDefaults.colors(
                        containerColor = androidx.compose.ui.graphics.Color.Transparent,
                    ),
                    supportingContent = {
                        Text(
                            o.lines.joinToString(", ") { it.description }
                                .ifBlank { stringResource(R.string.kiosk_order) },
                        )
                    },
                    trailingContent = {
                        Column(horizontalAlignment = Alignment.End) {
                            Text(Amounts.show(context, o.totalPxmr).primary)
                            Text(
                                stringResource(
                                    when (Orders.stateOf(context, o)) {
                                        // Seen and settled are different
                                        // words on purpose: one is a claim,
                                        // the other is the chain.
                                        Orders.State.Awaiting -> R.string.kiosk_state_awaiting
                                        Orders.State.Seen -> R.string.kiosk_state_seen
                                        Orders.State.Confirmed -> R.string.kiosk_state_confirmed
                                        Orders.State.Abandoned -> R.string.kiosk_state_abandoned
                                    },
                                ),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                            // Paid and waiting: the one message the counter
                            // owes somebody who stepped outside to wait.
                            if (o.personaHex != null && o.readyAt == 0L &&
                                Orders.stateOf(context, o) in
                                setOf(Orders.State.Seen, Orders.State.Confirmed)
                            ) {
                                TextButton(
                                    onClick = {
                                        scope.launch(Dispatchers.IO) {
                                            runCatching { Orders.sayReady(context, o) }
                                        }
                                    },
                                ) { Text(stringResource(R.string.kiosk_say_ready)) }
                            }
                            if (o.readyAt > 0L) {
                                Text(
                                    stringResource(R.string.kiosk_told_ready),
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.ducat.settled,
                                )
                            }
                            // Billed by mistake, or the customer changed their
                            // mind at the counter. Without this the shop could
                            // send a bill and never take it back, and the
                            // customer's phone kept a live "Review payment"
                            // pointing at money nobody was waiting for — which
                            // is the one way a person ends up paying for
                            // something that was cancelled out loud.
                            if (o.tabId != null &&
                                Orders.stateOf(context, o) == Orders.State.Awaiting
                            ) {
                                TextButton(
                                    onClick = {
                                        scope.launch(Dispatchers.IO) {
                                            runCatching {
                                                val tabs = TabStore(context)
                                                tabs.get(o.tabId!!)?.let { tabs.cancel(it) }
                                            }
                                        }
                                    },
                                ) { Text(stringResource(R.string.kiosk_withdraw)) }
                            }
                        }
                    },
                )
                HorizontalDivider()
            }
        }
    }
}
