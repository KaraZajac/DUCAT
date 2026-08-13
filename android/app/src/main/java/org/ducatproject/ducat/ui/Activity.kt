package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDownward
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Receipt
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.LOCK_BLOCKS
import org.ducatproject.ducat.Wallet
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.WalletStore
import org.ducatproject.ducat.formatXmr

/**
 * What has actually happened, from the chain.
 *
 * Mostly nameless by design: an output carries no sender, and §16.3 keeps a
 * transaction anonymous unless a contact was established. Attaching a name
 * where none exists would be inventing one.
 */
@Composable
fun ActivityScreen() {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    // Both directions. A list that only shows what arrived explains a rising
    // balance and leaves a falling one a mystery.
    val rows = remember(version) {
        val received = Wallet.entries(context).map { e ->
            Row(
                incoming = true,
                amountPxmr = e.amountPxmr,
                height = e.height,
                timestamp = 0,
                spent = e.spent,
                subtitle = null,
            )
        }
        val sent = WalletStore(context).sends().map { p ->
            Row(
                incoming = false,
                amountPxmr = p.amountPxmr,
                height = 0,
                timestamp = p.timestamp,
                spent = false,
                subtitle = buildString {
                    p.note?.takeIf { it.isNotBlank() }?.let { append(it).append(" · ") }
                    append("fee ${formatXmr(p.feePxmr)} XMR")
                },
            )
        }
        // Received rows carry a height and sent rows a clock; sorting by
        // whichever they have keeps them interleaved roughly in order.
        (received + sent).sortedByDescending { maxOf(it.height, it.timestamp) }
    }
    val tip = remember(version) { Wallet.balances(context).tip }

    if (rows.isEmpty()) {
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
        items(rows) { e ->
            val locked = e.incoming && tip > 0 && e.height > 0 && e.height + LOCK_BLOCKS > tip
            ListItem(
                headlineContent = {
                    val a = Amounts.show(context, e.amountPxmr)
                    Column {
                        Text("${if (e.incoming) "+" else "−"}${a.primary}")
                        a.secondary?.let {
                            Text(
                                it,
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.outline,
                            )
                        }
                    }
                },
                supportingContent = {
                    Text(
                        e.subtitle ?: buildString {
                            append("block ${e.height}")
                            if (e.spent) append(" · spent")
                            else if (locked) append(" · unlocks in ${e.height + LOCK_BLOCKS - tip} blocks")
                        },
                        style = MaterialTheme.typography.bodySmall,
                    )
                },
                leadingContent = {
                    Icon(
                        when {
                            !e.incoming -> Icons.Filled.ArrowUpward
                            locked -> Icons.Filled.Lock
                            else -> Icons.Filled.ArrowDownward
                        },
                        null,
                        tint = when {
                            !e.incoming -> MaterialTheme.colorScheme.onSurfaceVariant
                            e.spent -> MaterialTheme.colorScheme.outline
                            locked -> MaterialTheme.ducat.changePending
                            else -> MaterialTheme.ducat.settled
                        },
                    )
                },
            )
            HorizontalDivider()
        }
    }
}


/** One line in the list, from either direction. */
private data class Row(
    val incoming: Boolean,
    val amountPxmr: Long,
    val height: Long,
    val timestamp: Long,
    val spent: Boolean,
    val subtitle: String?,
)
