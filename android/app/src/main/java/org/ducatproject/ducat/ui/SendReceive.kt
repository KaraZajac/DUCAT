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
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.NodeStore
import org.ducatproject.ducat.Wallet
import org.ducatproject.ducat.WalletStore
import org.ducatproject.ducat.Amounts
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
fun SendReceiveSheet(
    prefillAddress: String? = null,
    prefillAmountPxmr: Long = 0,
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current
    val version by ContactStore.changes.collectAsState()
    val wallet = remember { WalletStore(context) }
    val b = remember(version) { Wallet.balances(context) }
    // Straight to Send when this was opened from a request: the user already
    // said what they wanted to do.
    var tab by remember { mutableIntStateOf(if (prefillAddress != null) 1 else 0) }
    var scanning by remember { mutableStateOf(false) }
    var dest by remember { mutableStateOf(prefillAddress.orEmpty()) }
    var amount by remember {
        mutableStateOf(
            if (prefillAmountPxmr > 0) formatXmr(prefillAmountPxmr) else ""
        )
    }
    var busy by remember { mutableStateOf(false) }
    var confirming by remember { mutableStateOf(false) }
    var sent by remember { mutableStateOf<uniffi.ducat_mobile.SendResult?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val pxmr = remember(amount) {
        amount.trim().toBigDecimalOrNull()
            ?.multiply(java.math.BigDecimal(1_000_000_000_000L))
            ?.toLong()?.takeIf { it > 0 }
    }

    if (scanning) {
        QrScanner(
            prompt = "Point the camera at a Monero address or a DUCAT card",
            onResult = { raw ->
                scanning = false
                // A `monero:` URI is what a wallet QR usually carries; the
                // address is the part after it.
                dest = raw.removePrefix("monero:").substringBefore("?")
            },
            onDismiss = { scanning = false },
        )
        return
    }

    if (confirming && pxmr != null) {
        // §15.5's checkpoint. The party whose money is at risk decides, and
        // nothing may shorten the path to here.
        AlertDialog(
            onDismissRequest = { confirming = false },
            title = {
                // The confirm always shows both. This is the moment someone
                // commits, and the unit they were reading a second ago must not
                // be the only one on the screen.
                val a = Amounts.show(context, pxmr)
                Column {
                    Text("Send ${a.primary}?")
                    a.secondary?.let {
                        Text(it, style = MaterialTheme.typography.labelMedium,
                             color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            },
            text = {
                Column {
                    Text("To", style = MaterialTheme.typography.labelMedium)
                    Text(dest, fontFamily = FontFamily.Monospace,
                         style = MaterialTheme.typography.bodySmall)
                    Spacer(Modifier.height(10.dp))
                    Text(
                        "Monero payments cannot be reversed or cancelled. Check the " +
                            "address — there is nobody to appeal to if it is wrong.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.ducat.changePending,
                    )
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    confirming = false
                    busy = true
                    error = null
                    scope.launch {
                        val node = NodeStore(context).lastGood()
                        val r = withContext(Dispatchers.IO) {
                            runCatching {
                                Wallet.send(
                                    context,
                                    node ?: throw IllegalStateException("no node — check Status"),
                                    dest.trim(),
                                    pxmr,
                                )
                            }
                        }
                        busy = false
                        r.onSuccess { sent = it; amount = ""; dest = "" }
                            .onFailure { error = it.message ?: "could not send" }
                    }
                }) { Text("Send") }
            },
            dismissButton = { TextButton(onClick = { confirming = false }) { Text("Cancel") } },
        )
    }

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
                        "Tap to pay over NFC. The AID is registered and " +
                            "the flow runs in the harness; the phone side is not wired.",
                    )
                }
            } else {
                Text("Send", style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(4.dp))
                val avail = Amounts.show(context, b.spendablePxmr)
                Text(
                    "Spendable now: ${avail.primary} across ${b.spendableOutputs} note(s)",
                    style = MaterialTheme.typography.bodyMedium,
                )
                avail.secondary?.let {
                    Text(it, style = MaterialTheme.typography.labelSmall,
                         color = MaterialTheme.colorScheme.outline)
                }
                if (b.lockedPxmr > 0) {
                    Text(
                        "${Amounts.show(context, b.lockedPxmr).primary} still locked — kept " +
                            "these apart because one is money you can hand over and " +
                            "the other is not.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.ducat.changePending,
                    )
                }
                Spacer(Modifier.height(16.dp))

                OutlinedTextField(
                    value = dest,
                    onValueChange = { dest = it },
                    label = { Text("To (Monero address)") },
                    modifier = Modifier.fillMaxWidth(),
                    minLines = 2,
                    trailingIcon = {
                        IconButton(onClick = { scanning = true }) {
                            Icon(Icons.Filled.QrCodeScanner, "Scan")
                        }
                    },
                )
                Spacer(Modifier.height(10.dp))
                OutlinedTextField(
                    value = amount,
                    onValueChange = { amount = it },
                    label = { Text("Amount (XMR)") },
                    singleLine = true,
                    isError = amount.isNotBlank() && pxmr == null,
                    modifier = Modifier.fillMaxWidth(),
                )

                val plan = remember(pxmr, version) {
                    pxmr?.let { Wallet.plan(context, it) }
                }
                plan?.let { p ->
                    Spacer(Modifier.height(10.dp))
                    Text(
                        if (p.enough) {
                            "Uses ${p.notes.size} note(s). The fee is added on top and " +
                                "is only known once the transaction is built."
                        } else {
                            "Not enough unlocked — ${Amounts.show(context, p.totalInPxmr).primary} available."
                        },
                        style = MaterialTheme.typography.bodySmall,
                        color = if (p.enough) MaterialTheme.colorScheme.onSurfaceVariant
                                else MaterialTheme.colorScheme.error,
                    )
                }

                Spacer(Modifier.height(16.dp))
                Button(
                    onClick = { confirming = true },
                    enabled = !busy && dest.isNotBlank() && (plan?.enough == true),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    if (busy) {
                        CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                        Spacer(Modifier.width(10.dp))
                        Text("Signing and sending…")
                    } else {
                        Text("Review")
                    }
                }

                sent?.let { r ->
                    Spacer(Modifier.height(14.dp))
                    Card(colors = CardDefaults.cardColors(
                        containerColor = MaterialTheme.colorScheme.surfaceVariant)) {
                        Column(Modifier.padding(14.dp)) {
                            Text("Sent", style = MaterialTheme.typography.labelLarge,
                                 color = MaterialTheme.ducat.settled)
                            Spacer(Modifier.height(4.dp))
                            Text(r.txidHex, fontFamily = FontFamily.Monospace,
                                 style = MaterialTheme.typography.bodySmall)
                            Spacer(Modifier.height(6.dp))
                            Text(
                                "fee ${Amounts.show(context, r.feePxmr.toLong()).primary} · " +
                                    "accepted by ${r.acceptedBy} node(s)",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
                error?.let {
                    Spacer(Modifier.height(12.dp))
                    Text(it, color = MaterialTheme.colorScheme.error,
                         style = MaterialTheme.typography.bodySmall)
                }
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
