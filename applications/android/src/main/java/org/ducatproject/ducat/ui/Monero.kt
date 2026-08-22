package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.NodeStore
import org.ducatproject.ducat.R
import org.ducatproject.ducat.SyncBlocker
import org.ducatproject.ducat.Wallet
import org.ducatproject.ducat.WalletStore
import org.ducatproject.ducat.saidWhy
import uniffi.ducat_mobile.MoneroNodeStatus
import uniffi.ducat_mobile.NodeTrust
import uniffi.ducat_mobile.moneroDefaultNodes
import uniffi.ducat_mobile.moneroPickNode

/**
 * Which Monero node we are using, and what it costs to use it.
 *
 * The panel names the trust level rather than a protocol. Dandelion++ runs in
 * the daemon and has done since 2019, so every live node does it and saying so
 * would distinguish nothing. What differs is whether the node is yours and what
 * carries the request — and that is what a person can actually act on.
 */
@Composable
fun MoneroPanel() {
    val context = LocalContext.current
    val store = remember { NodeStore(context) }
    val scope = rememberCoroutineScope()
    var status by remember { mutableStateOf<MoneroNodeStatus?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var editing by remember { mutableStateOf(false) }
    var ownUrl by remember { mutableStateOf(store.ownUrl() ?: "") }

    fun refresh() {
        busy = true
        error = null
        scope.launch {
            val r = withContext(Dispatchers.IO) {
                runCatching {
                    moneroPickNode(
                        moneroDefaultNodes(store.ownUrl()),
                        "stagenet",
                        8000u,
                    )
                }
            }
            busy = false
            r.onSuccess { status = it; store.rememberLastGood(it.url) }
                .onFailure {
                    status = null
                    error = it.saidWhy() ?: context.getString(R.string.monero_no_usable_node)
                }
        }
    }

    LaunchedEffect(Unit) { refresh() }

    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(R.string.monero_title), style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.weight(1f))
                AssistChip(onClick = {}, label = { Text("stagenet") })
                IconButton(onClick = { refresh() }, enabled = !busy) {
                    Icon(Icons.Filled.Refresh, stringResource(R.string.monero_check_again))
                }
            }
            Spacer(Modifier.height(4.dp))
            Text(
                stringResource(R.string.monero_subtitle),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(12.dp))

            when {
                busy && status == null -> Row(verticalAlignment = Alignment.CenterVertically) {
                    CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                    Spacer(Modifier.width(10.dp))
                    Text(stringResource(R.string.monero_finding_node))
                }
                status != null -> {
                    val s = status!!
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("✓", color = MaterialTheme.ducat.settled)
                        Spacer(Modifier.width(8.dp))
                        Text(s.url, fontFamily = FontFamily.Monospace,
                             style = MaterialTheme.typography.bodySmall)
                    }
                    Spacer(Modifier.height(8.dp))
                    Line(stringResource(R.string.monero_line_height), "${s.height}")
                    Line(
                        stringResource(R.string.monero_line_synced),
                        if (s.synced) stringResource(R.string.monero_synced_yes)
                        else stringResource(R.string.monero_synced_no),
                    )
                    Line(stringResource(R.string.monero_line_network), s.nettype)
                    Line(
                        stringResource(R.string.monero_line_round_trip),
                        stringResource(R.string.monero_round_trip_ms, "${s.rttMs}"),
                    )
                    Spacer(Modifier.height(10.dp))
                    TrustNote(trustOf(store.ownUrl(), s.url))
                }
                else -> Text(
                    error ?: stringResource(R.string.monero_no_node),
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
            }

            HorizontalDivider(Modifier.padding(vertical = 14.dp))
            WalletSync(status?.height?.toLong() ?: 0L)

            Spacer(Modifier.height(14.dp))
            if (!editing) {
                TextButton(onClick = { editing = true }) {
                    Text(
                        if (store.ownUrl() != null) stringResource(R.string.monero_change_your_node)
                        else stringResource(R.string.monero_use_own_node)
                    )
                }
            } else {
                OutlinedTextField(
                    value = ownUrl,
                    onValueChange = { ownUrl = it },
                    label = { Text(stringResource(R.string.monero_your_node)) },
                    placeholder = { Text("http://192.168.1.10:38081") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(8.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(
                        onClick = {
                            store.setOwnUrl(ownUrl.ifBlank { null })
                            editing = false
                            refresh()
                        },
                        // Not on an empty field. `ifBlank { null }` means a
                        // blank one sets *no* node — so "Use it" pressed with
                        // nothing typed quietly did what the button beside it
                        // says it does, and went back to the public node while
                        // claiming to use yours.
                        enabled = ownUrl.isNotBlank(),
                    ) { Text(stringResource(R.string.monero_use_it)) }
                    if (store.ownUrl() != null) {
                        OutlinedButton(onClick = {
                            store.setOwnUrl(null); ownUrl = ""; editing = false; refresh()
                        }) { Text(stringResource(R.string.monero_back_to_public)) }
                    }
                    TextButton(onClick = { editing = false }) { Text(stringResource(R.string.monero_cancel)) }
                }
            }
        }
    }
}

private fun trustOf(own: String?, inUse: String): NodeTrust = when {
    own != null && own.trim() == inUse -> NodeTrust.OWN
    inUse.contains(".onion") -> NodeTrust.ONION
    else -> NodeTrust.PUBLIC_CLEARNET
}

@Composable
private fun TrustNote(trust: NodeTrust) {
    val (title, body, colour) = when (trust) {
        NodeTrust.OWN -> Triple(
            stringResource(R.string.monero_trust_own_title),
            stringResource(R.string.monero_trust_own_body),
            MaterialTheme.ducat.settled,
        )
        NodeTrust.ONION -> Triple(
            stringResource(R.string.monero_trust_onion_title),
            stringResource(R.string.monero_trust_onion_body),
            MaterialTheme.colorScheme.onSurfaceVariant,
        )
        NodeTrust.PUBLIC_CLEARNET -> Triple(
            stringResource(R.string.monero_trust_public_title),
            stringResource(R.string.monero_trust_public_body),
            MaterialTheme.ducat.lowCapacity,
        )
    }
    Column {
        Text(title, style = MaterialTheme.typography.labelLarge, color = colour)
        Spacer(Modifier.height(2.dp))
        Text(
            body,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun Line(k: String, v: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 2.dp)) {
        Text(k, style = MaterialTheme.typography.bodySmall,
             color = MaterialTheme.colorScheme.onSurfaceVariant)
        Spacer(Modifier.weight(1f))
        Text(v, style = MaterialTheme.typography.bodySmall)
    }
}


/**
 * How far through the chain this wallet has read.
 *
 * Separate from the node's own sync, and the distinction matters: a node can be
 * fully caught up while the wallet has read none of it. Showing only the node's
 * state — which is what this panel did — tells someone everything is fine while
 * their balance is still zero.
 */
@Composable
private fun WalletSync(nodeHeight: Long) {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val b = remember(version, nodeHeight) { Wallet.balances(context) }
    var rescanOpen by remember { mutableStateOf(false) }

    Text(stringResource(R.string.monero_wallet_title), style = MaterialTheme.typography.titleSmall)
    Spacer(Modifier.height(6.dp))

    when {
        b.tip == 0L -> when (remember(version) { Wallet.blocker(context) }) {
            SyncBlocker.NoWallet -> Column {
                Text(
                    stringResource(R.string.monero_no_wallet_key_title),
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.error,
                )
                Spacer(Modifier.height(4.dp))
                Text(
                    stringResource(R.string.monero_no_wallet_key_body),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            SyncBlocker.Failing -> Column {
                Text(
                    stringResource(R.string.monero_scanning_failed),
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.error,
                )
                Spacer(Modifier.height(4.dp))
                Text(
                    Wallet.lastError(context) ?: stringResource(R.string.monero_unknown),
                    style = MaterialTheme.typography.bodySmall,
                    fontFamily = FontFamily.Monospace,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(6.dp))
                Text(
                    stringResource(R.string.monero_scanning_failed_note),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            else -> Text(
                stringResource(R.string.monero_not_started),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        !b.syncing -> Row(verticalAlignment = Alignment.CenterVertically) {
            Text("✓", color = MaterialTheme.ducat.settled)
            Spacer(Modifier.width(8.dp))
            Text(
                stringResource(R.string.monero_caught_up, b.tip),
                style = MaterialTheme.typography.bodySmall,
            )
        }

        else -> Column {
            LinearProgressIndicator(
                progress = { b.progress },
                modifier = Modifier.fillMaxWidth().height(6.dp),
            )
            Spacer(Modifier.height(8.dp))
            Row {
                Text(
                    stringResource(R.string.monero_block_of, b.scannedTo, b.tip),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.weight(1f))
                Text(
                    "${(b.progress * 100).toInt()}%",
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            Spacer(Modifier.height(4.dp))
            Text(
                buildString {
                    append(
                        pluralStringResource(
                            R.plurals.monero_blocks_to_go,
                            b.blocksLeft.toInt(), b.blocksLeft,
                        )
                    )
                    // Only when it has been measured. A made-up estimate is
                    // worse than none, because people plan around it.
                    b.secondsLeft?.let { append(" · ${humanDuration(context, it)}") }
                },
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(6.dp))
            Text(
                stringResource(R.string.monero_partial_balance),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.ducat.changePending,
            )

            // A stall after progress showed a frozen bar and nothing else: the
            // failure branch only ran when nothing had been read at all, so the
            // one case with a visible symptom had no explanation attached.
            b.error?.let { e ->
                Spacer(Modifier.height(8.dp))
                Text(
                    stringResource(R.string.monero_last_window_failed, e),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
            }

            // The escape hatch belongs here too: this is the screen someone
            // watching a stuck scan is already looking at.
            if (b.scannedTo in 1..(b.tip - 100_000)) {
                Spacer(Modifier.height(10.dp))
                Text(
                    stringResource(R.string.monero_rescan_hint),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
                Spacer(Modifier.height(6.dp))
                OutlinedButton(onClick = { rescanOpen = true }) { Text(stringResource(R.string.monero_skip_ahead)) }
            }
        }
    }

    if (rescanOpen) {
        SkipAheadDialog(
            tip = b.tip,
            onPick = { WalletStore(context).rescanFrom(it); rescanOpen = false },
            onDismiss = { rescanOpen = false },
        )
    }
}
