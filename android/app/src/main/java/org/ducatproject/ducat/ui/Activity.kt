package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDownward
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
    val entries = remember(version) { Wallet.entries(context) }
    val tip = remember(version) { Wallet.balances(context).tip }

    if (entries.isEmpty()) {
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
        items(entries) { e ->
            val locked = tip > 0 && e.height + LOCK_BLOCKS > tip
            ListItem(
                headlineContent = {
                    val a = Amounts.show(context, e.amountPxmr)
                    Column {
                        Text("+${a.primary}")
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
                        buildString {
                            append("block ${e.height}")
                            if (e.spent) append(" · spent")
                            else if (locked) append(" · unlocks in ${e.height + LOCK_BLOCKS - tip} blocks")
                        },
                        style = MaterialTheme.typography.bodySmall,
                    )
                },
                leadingContent = {
                    Icon(
                        if (locked) Icons.Filled.Lock else Icons.Filled.ArrowDownward,
                        null,
                        tint = when {
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
