package org.ducatproject.ducat.ui

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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.BillItem
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mode
import org.ducatproject.ducat.ModeStore
import org.ducatproject.ducat.Orders
import org.ducatproject.ducat.R
import org.ducatproject.ducat.formatXmr

/**
 * The counter, facing the other way (§15.11's modes, turned outward).
 *
 * Every other screen in this app belongs to the person holding the phone.
 * This one belongs to a stranger standing in front of it: they tap what they
 * want, they pay with whatever Monero wallet they already have, and they
 * never touch anything else. No contact is made, no card is exchanged, no
 * conversation is opened — a queue for coffee has no time for any of it.
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
    var placed by remember { mutableStateOf<Orders.Order?>(null) }
    var staffDoor by remember { mutableStateOf(false) }
    var staffOpen by remember { mutableStateOf(false) }

    // While an order is on screen, watch for its payment. The poller does the
    // same sweep for the shop as a whole; this is the same question asked
    // often enough that a person standing at a counter sees the answer.
    LaunchedEffect(placed?.id, version) {
        val waiting = placed ?: return@LaunchedEffect
        if (waiting.state != Orders.State.Awaiting) return@LaunchedEffect
        while (true) {
            kotlinx.coroutines.delay(3_000)
            val now = Orders.all(context).firstOrNull { it.id == waiting.id } ?: return@LaunchedEffect
            if (now.state != Orders.State.Awaiting) {
                placed = now
                return@LaunchedEffect
            }
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
                    order = placed!!,
                    onDone = { placed = null; basket = emptyList() },
                )
                else -> Ordering(
                    basket = basket,
                    onAdd = { name, pxmr -> basket = basket + BillItem(name, pxmr) },
                    onClear = { basket = emptyList() },
                    onOrder = { placed = Orders.place(context, basket) },
                )
            }
        }
    }

    PinGate(
        open = staffDoor,
        onDismiss = { staffDoor = false },
        onPassed = { staffDoor = false; staffOpen = true },
    )
}

/** Tap what you want; see what it comes to. */
@Composable
private fun Ordering(
    basket: List<BillItem>,
    onAdd: (String, Long) -> Unit,
    onClear: () -> Unit,
    onOrder: () -> Unit,
) {
    val context = LocalContext.current
    val total = basket.sumOf { it.amountPxmr }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Spacer(Modifier.height(8.dp))
        ItemPicker(onPick = onAdd)
        Spacer(Modifier.height(16.dp))
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

/** The code to pay, and then the word that the money arrived. */
@Composable
private fun PayPanel(order: Orders.Order, onDone: () -> Unit) {
    val context = LocalContext.current
    val paid = order.state != Orders.State.Awaiting
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
                stringResource(R.string.kiosk_paid_number, order.number),
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
            Amounts.show(context, order.totalPxmr).primary,
            style = MaterialTheme.typography.headlineMedium,
        )
        Text(
            "${formatXmr(order.totalPxmr)} XMR",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(12.dp))
        QrBlock(Orders.payUri(order))
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
private fun StaffPanel(onClose: () -> Unit) {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val orders = remember(version) { Orders.all(context) }
    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                stringResource(R.string.kiosk_orders),
                Modifier.weight(1f),
                style = MaterialTheme.typography.titleLarge,
            )
            IconButton(onClick = onClose) {
                Icon(Icons.Filled.Close, stringResource(R.string.kiosk_back_to_counter))
            }
        }
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
                                    when (o.state) {
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
                        }
                    },
                )
                HorizontalDivider()
            }
        }
    }
}
