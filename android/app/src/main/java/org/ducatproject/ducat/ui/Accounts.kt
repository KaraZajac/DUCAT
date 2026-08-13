package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.QrCode2
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.Wallet
import org.ducatproject.ducat.WalletStore
import org.ducatproject.ducat.formatXmr

/**
 * The money side.
 *
 * The address shown here is an **ordinary Monero address**, and that is the
 * point: topping up should work from any wallet, any exchange, anything that
 * can send XMR. DUCAT's protocol has no part in funding it — §17.2's float is
 * just outputs this key controls.
 *
 * §17.2 also forbids showing float, reserve and bond as one number, which is
 * why they are three rows rather than a balance.
 */
@Composable
fun AccountsScreen() {
    val context = LocalContext.current
    // Recomputed on every store change, which the poller bumps after each scan
    // window — so the figure climbs while syncing rather than appearing at the
    // end.
    val version by org.ducatproject.ducat.ContactStore.changes.collectAsState()
    val balances = remember(version) { Wallet.balances(context) }
    var preferFiat by remember(version) { mutableStateOf(Amounts.preferFiat(context)) }
    val clipboard = LocalClipboardManager.current
    val wallet = remember { WalletStore(context) }
    val address = wallet.address()
    var showQr by remember { mutableStateOf(false) }
    var rescanOpen by remember { mutableStateOf(false) }

    if (rescanOpen) {
        SkipAheadDialog(
            tip = balances.tip,
            onPick = {
                WalletStore(context).rescanFrom(it)
                rescanOpen = false
            },
            onDismiss = { rescanOpen = false },
        )
    }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp)) {
        Card(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(16.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("Top up", style = MaterialTheme.typography.titleMedium)
                    Spacer(Modifier.weight(1f))
                    if (wallet.stagenet()) {
                        AssistChip(onClick = {}, label = { Text("stagenet") })
                    }
                }
                Spacer(Modifier.height(6.dp))
                Text(
                    "Send Monero to this address from any wallet or exchange. " +
                        "It is a normal XMR address — nothing about the transfer " +
                        "involves DUCAT.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(14.dp))

                if (address == null) {
                    Text(
                        "No wallet yet — finish setup first.",
                        color = MaterialTheme.colorScheme.error,
                    )
                } else {
                    SelectionContainer {
                        Text(
                            address,
                            fontFamily = FontFamily.Monospace,
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                    Spacer(Modifier.height(14.dp))
                    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        Button(
                            onClick = { clipboard.setText(AnnotatedString(address)) },
                            modifier = Modifier.weight(1f),
                        ) {
                            Icon(Icons.Filled.ContentCopy, null, Modifier.size(18.dp))
                            Spacer(Modifier.width(8.dp))
                            Text("Copy")
                        }
                        OutlinedButton(
                            onClick = { showQr = !showQr },
                            modifier = Modifier.weight(1f),
                        ) {
                            Icon(Icons.Filled.QrCode2, null, Modifier.size(18.dp))
                            Spacer(Modifier.width(8.dp))
                            Text(if (showQr) "Hide" else "QR")
                        }
                    }
                    if (showQr) {
                        Spacer(Modifier.height(14.dp))
                        // `monero:` so any wallet app recognises it on scan.
                        QrBlock("monero:$address")
                    }
                }
            }
        }

        Spacer(Modifier.height(16.dp))
        Card(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(16.dp)) {
                Text("Balances", style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(4.dp))
                Text(
                    "§17.2 keeps these apart on purpose: what you can spend now is " +
                        "not what you own.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(12.dp))
                val b = balances
                if (b.tip == 0L) {
                    Text(
                        "Waiting for a node.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                } else {
                    val spend = Amounts.show(context, b.spendablePxmr, wallet.stagenet())
                    BalanceRow("Spendable", spend.primary, spend.secondary)
                    if (Amounts.canConvert(context)) {
                        Spacer(Modifier.height(4.dp))
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(
                                "Show in ${Amounts.currency(context)}",
                                style = MaterialTheme.typography.bodySmall,
                            )
                            Spacer(Modifier.weight(1f))
                            Switch(
                                checked = preferFiat,
                                onCheckedChange = {
                                    Amounts.setPreferFiat(context, it); preferFiat = it
                                },
                            )
                        }
                        if (preferFiat && wallet.stagenet()) {
                            Text(
                                "Stagenet coins are test coins. That figure is what this " +
                                    "much real Monero would be worth — these are worth nothing.",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.ducat.changePending,
                            )
                        }
                        Spacer(Modifier.height(6.dp))
                    }
                    BalanceRow("Notes", "${b.spendableOutputs}")
                    BalanceRow("Bond", "none")
                    // A wallet whose restore height is zero is scanning from
                    // genesis, which looks exactly like having no money for the
                    // day and a half it takes to arrive.
                    if (b.scannedTo in 1..(b.tip - 100_000)) {
                        Spacer(Modifier.height(12.dp))
                        Card(colors = CardDefaults.cardColors(
                            containerColor = MaterialTheme.colorScheme.errorContainer)) {
                            Column(Modifier.padding(12.dp)) {
                                Text(
                                    "Scanning from the beginning of the chain",
                                    style = MaterialTheme.typography.labelLarge,
                                )
                                Spacer(Modifier.height(4.dp))
                                Text(
                                    "This wallet was made before the app could reach a " +
                                        "node, so it does not know when it was created. " +
                                        "At block ${b.scannedTo} of ${b.tip} that is more " +
                                        "than a day of reading. Skip ahead if you know the " +
                                        "wallet is newer than that.",
                                    style = MaterialTheme.typography.bodySmall,
                                )
                                Spacer(Modifier.height(10.dp))
                                Button(onClick = { rescanOpen = true }) { Text("Skip ahead") }
                            }
                        }
                    }
                    // Shared with Home rather than reimplemented. The copy that
                    // used to live here measured progress as scannedTo/tip —
                    // the bug already fixed in `Balances.progress` — so this
                    // screen sat at 100% for a scan that had just begun.
                    // The spacer belongs to the status, not to the card: with
                    // no node yet SyncStatus draws nothing, and an unconditional
                    // gap left the card ending in blank space.
                    if (b.tip > 0) {
                        Spacer(Modifier.height(12.dp))
                        SyncStatus(b)
                    }
                }
            }
        }
    }
}

@Composable
private fun BalanceRow(label: String, value: String, secondary: String? = null) {
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
        Text(label, style = MaterialTheme.typography.bodyMedium)
        Spacer(Modifier.weight(1f))
        Column(horizontalAlignment = Alignment.End) {
            Text(
                value,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            // The other unit, always. A converted figure alone is a figure
            // nobody can check.
            secondary?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.outline,
                )
            }
        }
    }
}


/**
 * Where to start reading from.
 *
 * The default is a day back rather than the tip: starting *at* the tip skips
 * anything that arrived while the wallet was not looking, and a wallet that
 * silently misses a payment is worse than one that takes a few minutes longer.
 */
@Composable
internal fun SkipAheadDialog(tip: Long, onPick: (Long) -> Unit, onDismiss: () -> Unit) {
    val suggestions = listOf(
        720L to "about a day ago",
        5_040L to "about a week ago",
        21_600L to "about a month ago",
    )
    var custom by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Skip ahead to") },
        text = {
            Column {
                Text(
                    "Anything received before this point will not be found. Pick a " +
                        "time you are sure is before the wallet existed.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(12.dp))
                suggestions.forEach { (back, label) ->
                    val h = (tip - back).coerceAtLeast(0)
                    TextButton(onClick = { onPick(h) }, modifier = Modifier.fillMaxWidth()) {
                        Text("$label  —  block $h", modifier = Modifier.fillMaxWidth())
                    }
                }
                Spacer(Modifier.height(8.dp))
                OutlinedTextField(
                    value = custom,
                    onValueChange = { custom = it.filter { c -> c.isDigit() } },
                    label = { Text("or a block height") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = { custom.toLongOrNull()?.let(onPick) },
                enabled = custom.toLongOrNull()?.let { it in 0..tip } == true,
            ) { Text("Use that height") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}
