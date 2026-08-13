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
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
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
    val clipboard = LocalClipboardManager.current
    val wallet = remember { WalletStore(context) }
    val address = wallet.address()
    var showQr by remember { mutableStateOf(false) }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp)) {
        Card(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(16.dp)) {
                Row(verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
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
                    BalanceRow("Spendable", "${formatXmr(b.spendablePxmr)} XMR")
                    if (b.lockedPxmr > 0) {
                        BalanceRow(
                            "Locked",
                            "${formatXmr(b.lockedPxmr)} XMR · ~${b.blocksToUnlock * 2} min",
                        )
                    }
                    BalanceRow("Notes", "${b.spendableOutputs}")
                    BalanceRow("Bond", "none")
                    if (b.syncing) {
                        Spacer(Modifier.height(10.dp))
                        val done = (b.scannedTo - 0).coerceAtLeast(0)
                        Text(
                            "Reading the chain — block $done of ${b.tip}. " +
                                "A balance shown before this finishes is only what " +
                                "has been seen so far.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.ducat.changePending,
                        )
                        Spacer(Modifier.height(6.dp))
                        LinearProgressIndicator(
                            progress = { if (b.tip > 0) (b.scannedTo.toFloat() / b.tip) else 0f },
                            modifier = Modifier.fillMaxWidth(),
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun BalanceRow(label: String, value: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
        Text(label, style = MaterialTheme.typography.bodyMedium)
        Spacer(Modifier.weight(1f))
        Text(
            value,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
