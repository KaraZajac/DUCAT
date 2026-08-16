package org.ducatproject.ducat.ui

import android.content.Context
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
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Ledger
import org.ducatproject.ducat.NodeStore
import org.ducatproject.ducat.R
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
    val pending = remember(version) { Ledger.openRequests(context) }
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
            Text(stringResource(R.string.activity_nothing_yet), style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(6.dp))
            Text(
                stringResource(R.string.activity_empty_desc),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        return
    }

    LazyColumn(Modifier.fillMaxSize()) {
        // The uncleared half of the statement: money asked for and not yet
        // moved, in either direction. A bank shows pending card holds for the
        // same reason — the balance alone under-describes the situation.
        if (pending.isNotEmpty()) {
            item {
                Text(
                    stringResource(R.string.activity_awaiting),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(start = 16.dp, top = 12.dp, bottom = 4.dp),
                )
            }
            items(pending) { r ->
                val shown = Amounts.show(context, r.amountPxmr)
                ListItem(
                    modifier = Modifier.clickable {
                        org.ducatproject.ducat.MainActivity.openChat.value = r.contactHex
                    },
                    colors = ListItemDefaults.colors(
                        containerColor = androidx.compose.ui.graphics.Color.Transparent,
                    ),
                    leadingContent = {
                        Icon(
                            Icons.Filled.Schedule, null,
                            tint = MaterialTheme.colorScheme.outline,
                        )
                    },
                    headlineContent = {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(
                                if (r.theyAsked)
                                    stringResource(R.string.activity_they_ask, r.counterparty)
                                else stringResource(R.string.activity_you_asked, r.counterparty),
                                style = MaterialTheme.typography.bodyMedium,
                            )
                            Spacer(Modifier.weight(1f))
                            Text(shown.primary, fontWeight = FontWeight.SemiBold)
                        }
                    },
                    supportingContent = {
                        Text(
                            (if (r.items.isNotEmpty())
                                pluralStringResource(
                                    R.plurals.activity_items, r.items.size, r.items.size,
                                ) + " · " else "") + whenText(context, r.timestamp),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.outline,
                        )
                    },
                )
            }
            item {
                HorizontalDivider(
                    color = MaterialTheme.colorScheme.outlineVariant,
                    modifier = Modifier.padding(vertical = 4.dp),
                )
            }
        }
        items(events) { e -> EventRow(e) { open = e } }
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
        // On the page itself. ListItem's default fill is a tonal band, and a
        // list of bands reads as a table; rows on the background with space
        // between them read as a feed, which is the register every payments
        // app the user knows writes in. The ruling lines went for the same
        // reason — the avatars already separate the rows.
        colors = ListItemDefaults.colors(containerColor = androidx.compose.ui.graphics.Color.Transparent),
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
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        if (e.provisional) stringResource(R.string.activity_reading_transaction)
                        else who(context, e),
                        style = MaterialTheme.typography.bodySmall,
                        color = if (e.provisional) MaterialTheme.ducat.changePending
                        else MaterialTheme.colorScheme.onSurface,
                    )
                    if (e.receipted) {
                        Spacer(Modifier.width(6.dp))
                        // The paperwork chip: this payment has a receipt in a
                        // thread, which is what separates a DUCAT payment from
                        // a bare transfer.
                        Text(
                            stringResource(R.string.activity_receipt_chip),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.ducat.settled,
                        )
                    }
                }
                e.note?.let {
                    Text(
                        stringResource(R.string.activity_note_quoted, it),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                    )
                }
                Text(
                    when {
                        e.pending -> stringResource(R.string.activity_sending)
                        e.unexplained -> stringResource(R.string.activity_spent_unknown)
                        // Until the transaction is read, this could be change
                        // from a payment you made rather than money arriving.
                        e.provisional -> stringResource(R.string.activity_maybe_change)
                        e.locked -> stringResource(
                            R.string.activity_locked_when,
                            whenText(context, e.timestamp), e.unlocksInBlocks,
                        )
                        else -> whenText(context, e.timestamp)
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
private fun who(context: Context, e: Ledger.Event): String {
    val sent = e.direction == Ledger.Direction.Sent
    e.counterparty?.let {
        return if (sent) context.getString(R.string.activity_to, it)
        else context.getString(R.string.activity_from, it)
    }
    e.address?.let {
        return context.getString(R.string.activity_to, "${it.take(12)}…${it.takeLast(6)}")
    }
    return if (sent) context.getString(R.string.activity_sent_unrecorded)
    else context.getString(R.string.activity_received_unknown)
}

/** A date and a time, because a block height is not a moment to anybody. */
internal fun whenText(context: Context, epochSecs: Long): String {
    if (epochSecs <= 0) return context.getString(R.string.txdetail_time_unknown)
    val d = java.util.Date(epochSecs * 1000)
    return java.text.SimpleDateFormat("d MMM yyyy, HH:mm", java.util.Locale.getDefault()).format(d)
}
