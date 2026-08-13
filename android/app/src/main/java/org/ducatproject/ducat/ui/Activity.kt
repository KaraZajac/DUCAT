package org.ducatproject.ducat.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDownward
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.filled.HelpOutline
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Receipt
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Ledger
import org.ducatproject.ducat.NodeStore
import org.ducatproject.ducat.Wallet

/**
 * What has actually happened, as transactions rather than outputs.
 *
 * The distinction is the whole screen. See [Ledger]: a wallet stores outputs,
 * and reading them out directly shows a send's change as income, hides the send
 * itself, and reports a total nobody can reconcile against the balance.
 *
 * Every row carries the balance *after* it, so the column adds up to the number
 * on Accounts. That is not decoration — it is the only way someone can check
 * that this screen and that one are describing the same money.
 */
@Composable
fun ActivityScreen() {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val events = remember(version) { Ledger.build(context) }
    val tip = remember(version) { Wallet.balances(context).tip }
    var open by remember { mutableStateOf<Ledger.Event?>(null) }

    // Ask the chain about anything still unclassified as soon as this screen is
    // looked at, rather than waiting up to a poll interval. Until a transaction
    // has been read, change from an outgoing payment is indistinguishable from
    // an incoming one, and this is the screen where that distinction is the
    // entire content.
    LaunchedEffect(version) {
        // Only what a fetch could actually resolve. Keying on "chain == null"
        // would also match rows that have no transaction id to look up, so the
        // condition would never go false and the screen would re-enrich on
        // every unrelated change to the store.
        if (events.any { it.provisional }) {
            withContext(Dispatchers.IO) {
                NodeStore(context).lastGood()?.let { Ledger.enrich(context, it) }
            }
        }
    }

    open?.let { e ->
        TxDetailScreen(e, tip) { open = null }
        return
    }

    if (events.isEmpty()) {
        Column(
            Modifier.fillMaxSize().padding(32.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Icon(
                Icons.Filled.Receipt, null, Modifier.size(48.dp),
                tint = MaterialTheme.colorScheme.outline,
            )
            Spacer(Modifier.height(12.dp))
            Text("Nothing yet", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(6.dp))
            Text(
                "Payments appear here once the chain has been read. Top up from " +
                    "Accounts to see something.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        return
    }

    LazyColumn(Modifier.fillMaxSize()) {
        items(events) { e -> EventRow(e) { open = e }; HorizontalDivider() }
    }
}

@Composable
private fun EventRow(e: Ledger.Event, onClick: () -> Unit) {
    val context = LocalContext.current
    val sent = e.direction == Ledger.Direction.Sent
    val amount = Amounts.show(context, e.amountPxmr)
    val balance = Amounts.show(context, e.balanceAfterPxmr)

    ListItem(
        modifier = Modifier.clickable(onClick = onClick),
        leadingContent = {
            Icon(
                when {
                    e.pending -> Icons.Filled.Schedule
                    e.unexplained -> Icons.Filled.HelpOutline
                    sent -> Icons.Filled.ArrowUpward
                    e.locked -> Icons.Filled.Lock
                    else -> Icons.Filled.ArrowDownward
                },
                null,
                tint = when {
                    e.pending -> MaterialTheme.colorScheme.outline
                    e.unexplained -> MaterialTheme.colorScheme.error
                    // Out and in are different directions *and* different
                    // colours. Both arrows pointed the same way before, which
                    // made a send indistinguishable from a receipt at a glance.
                    sent -> MaterialTheme.ducat.changePending
                    e.locked -> MaterialTheme.ducat.changePending
                    else -> MaterialTheme.ducat.settled
                },
            )
        },
        headlineContent = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    "${if (sent) "−" else "+"}${amount.primary}",
                    fontWeight = FontWeight.SemiBold,
                )
                Spacer(Modifier.weight(1f))
                // The balance after this row, so the column reconciles with
                // Accounts. Dimmer than the amount: it is context, not the event.
                if (!e.pending) {
                    Text(
                        balance.primary,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        },
        supportingContent = {
            Column {
                Row {
                    amount.secondary?.let {
                        Text(
                            it,
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.outline,
                        )
                    }
                    Spacer(Modifier.weight(1f))
                    if (!e.pending) {
                        balance.secondary?.let {
                            Text(
                                it,
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.outline,
                            )
                        }
                    }
                }
                Spacer(Modifier.height(2.dp))
                Text(
                    if (e.provisional) "Reading the transaction…" else who(e),
                    style = MaterialTheme.typography.bodySmall,
                    color = if (e.provisional) MaterialTheme.ducat.changePending
                    else MaterialTheme.colorScheme.onSurface,
                )
                Text(
                    when {
                        e.pending -> "Sending — not yet in a block"
                        e.unexplained -> "Spent, but we cannot say where"
                        // Until the transaction is read, this could be change
                        // from a payment you made rather than money arriving.
                        e.provisional -> "may be change from your own payment"
                        e.locked -> "${whenText(e.timestamp)} · unlocks in ${e.unlocksInBlocks} blocks"
                        else -> whenText(e.timestamp)
                    },
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.outline,
                )
            }
        },
    )
}

/**
 * Who was on the other side, and never a guess.
 *
 * A received payment usually has no answer: Monero does not carry a sender, and
 * the only thing that can supply one is a contact's own notice naming the
 * transaction (§16.13). Saying "unknown sender" is the honest result, and
 * inventing anything else would be inventing a counterparty.
 */
private fun who(e: Ledger.Event): String {
    val sent = e.direction == Ledger.Direction.Sent
    e.counterparty?.let { return if (sent) "To $it" else "From $it" }
    e.address?.let { return "To ${it.take(12)}…${it.takeLast(6)}" }
    return if (sent) "Sent — recipient not recorded" else "Received — sender unknown"
}

/** A date and a time, because a block height is not a moment to anybody. */
internal fun whenText(epochSecs: Long): String {
    if (epochSecs <= 0) return "time unknown"
    val d = java.util.Date(epochSecs * 1000)
    return java.text.SimpleDateFormat("d MMM yyyy, HH:mm", java.util.Locale.getDefault()).format(d)
}
