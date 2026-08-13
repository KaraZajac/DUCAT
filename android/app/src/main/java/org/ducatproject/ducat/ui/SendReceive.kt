package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Nfc
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Wallet
import org.ducatproject.ducat.WalletStore
import org.ducatproject.ducat.formatXmr

/**
 * The one verb that dominates: §15.2's `presenter_role`.
 *
 * **Request** means I present and you tap me — the POS direction. **Send**
 * means I read your tap. They are not symmetric: the presenter supplies
 * reachability, so the reader drives every round trip, and a screen written for
 * one and reused for the other hangs.
 *
 * Receive works today because it needs nothing but an address. Send needs
 * transaction construction, which is not built — and says so rather than
 * offering a form that fails at the end.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SendReceiveSheet(onDismiss: () -> Unit) {
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current
    val version by ContactStore.changes.collectAsState()
    val wallet = remember { WalletStore(context) }
    val b = remember(version) { Wallet.balances(context) }
    var tab by remember { mutableIntStateOf(0) }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(Modifier.padding(20.dp).verticalScroll(rememberScrollState())) {
            TabRow(selectedTabIndex = tab) {
                Tab(selected = tab == 0, onClick = { tab = 0 }, text = { Text("Request") })
                Tab(selected = tab == 1, onClick = { tab = 1 }, text = { Text("Send") })
            }
            Spacer(Modifier.height(20.dp))

            if (tab == 0) {
                val addr = wallet.address()
                Text("Receive Monero", style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(4.dp))
                Text(
                    "Anyone can pay this from any wallet. Nothing about the " +
                        "transfer involves DUCAT.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(16.dp))
                if (addr == null) {
                    Text("No wallet yet — finish setup first.",
                         color = MaterialTheme.colorScheme.error)
                } else {
                    QrBlock("monero:$addr")
                    Spacer(Modifier.height(14.dp))
                    SelectionContainer {
                        Text(addr, fontFamily = FontFamily.Monospace,
                             style = MaterialTheme.typography.bodySmall)
                    }
                    Spacer(Modifier.height(14.dp))
                    Button(
                        onClick = { clipboard.setText(AnnotatedString(addr)) },
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Icon(Icons.Filled.ContentCopy, null, Modifier.size(18.dp))
                        Spacer(Modifier.width(8.dp))
                        Text("Copy address")
                    }
                    Spacer(Modifier.height(16.dp))
                    NotBuilt(
                        Icons.Filled.Nfc,
                        "Tap to be paid",
                        "§15.2's presenter role over NFC. The AID is registered and " +
                            "the flow runs in the harness; the phone side is not wired.",
                    )
                }
            } else {
                Text("Send", style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(4.dp))
                Text(
                    "Spendable now: ${formatXmr(b.spendablePxmr)} XMR across " +
                        "${b.spendableOutputs} note(s)",
                    style = MaterialTheme.typography.bodyMedium,
                )
                if (b.lockedPxmr > 0) {
                    Text(
                        "${formatXmr(b.lockedPxmr)} XMR still locked — §17.2 keeps " +
                            "these apart because one is money you can hand over and " +
                            "the other is not.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.ducat.changePending,
                    )
                }
                Spacer(Modifier.height(20.dp))
                NotBuilt(
                    Icons.Filled.QrCodeScanner,
                    "Sending is not built yet",
                    "Reading the chain works; spending needs transaction " +
                        "construction and broadcast, which is a larger step. A form " +
                        "here would take an amount and fail at the end.",
                )
            }
            Spacer(Modifier.height(24.dp))
        }
    }
}

/**
 * An honest gap.
 *
 * Named and explained rather than hidden, because a control that looks
 * available and does nothing costs a user more than an absent one — and on a
 * payments screen, more than that.
 */
@Composable
private fun NotBuilt(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    title: String,
    why: String,
) {
    Card(colors = CardDefaults.cardColors(
        containerColor = MaterialTheme.colorScheme.surfaceVariant)) {
        Row(Modifier.padding(14.dp), verticalAlignment = Alignment.Top) {
            Icon(icon, null, tint = MaterialTheme.colorScheme.outline)
            Spacer(Modifier.width(12.dp))
            Column {
                Text(title, style = MaterialTheme.typography.labelLarge)
                Spacer(Modifier.height(4.dp))
                Text(
                    why,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}
