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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.R
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
                    Text(stringResource(R.string.accounts_top_up), style = MaterialTheme.typography.titleMedium)
                    Spacer(Modifier.weight(1f))
                    if (wallet.stagenet()) {
                        AssistChip(onClick = {}, label = { Text("stagenet") })
                    }
                }
                Spacer(Modifier.height(6.dp))
                Text(
                    stringResource(R.string.accounts_top_up_desc),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(14.dp))

                if (address == null) {
                    Text(
                        stringResource(R.string.accounts_no_wallet),
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
                            onClick = {
                                copyText(context, address, context.getString(R.string.accounts_copied))
                            },
                            modifier = Modifier.weight(1f),
                        ) {
                            Icon(Icons.Filled.ContentCopy, null, Modifier.size(18.dp))
                            Spacer(Modifier.width(8.dp))
                            Text(stringResource(R.string.accounts_copy))
                        }
                        OutlinedButton(
                            onClick = { showQr = !showQr },
                            modifier = Modifier.weight(1f),
                        ) {
                            Icon(Icons.Filled.QrCode2, null, Modifier.size(18.dp))
                            Spacer(Modifier.width(8.dp))
                            Text(
                                if (showQr) stringResource(R.string.accounts_hide)
                                else stringResource(R.string.accounts_qr)
                            )
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
                Text(stringResource(R.string.accounts_balances), style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(4.dp))
                Text(
                    stringResource(R.string.accounts_balances_desc),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(12.dp))
                val b = balances
                if (b.tip == 0L) {
                    Text(
                        stringResource(R.string.accounts_waiting_for_node),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                } else {
                    val spend = Amounts.show(context, b.spendablePxmr, wallet.stagenet())
                    BalanceRow(stringResource(R.string.accounts_spendable), spend.primary, spend.secondary)
                    // The card's own subtitle promises that what you can spend
                    // is not what you own, and then this screen used to show a
                    // single figure — leaving money that had arrived but not
                    // settled invisible on the one screen titled "Balances".
                    // Same sentence the balance card uses, so the two screens
                    // agree rather than each inventing a word for it.
                    if (b.lockedPxmr > 0) {
                        Text(
                            stringResource(
                                R.string.balance_arriving,
                                Amounts.show(context, b.lockedPxmr, wallet.stagenet()).primary,
                                minutesFor(context, b.blocksToUnlock.toInt()),
                            ),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.ducat.changePending,
                        )
                    }
                    if (Amounts.canConvert(context)) {
                        Spacer(Modifier.height(4.dp))
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(
                                stringResource(R.string.accounts_show_in, Amounts.currency(context)),
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
                                stringResource(R.string.accounts_stagenet_fiat_note),
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.ducat.changePending,
                            )
                        }
                        Spacer(Modifier.height(6.dp))
                    }
                    BalanceRow(stringResource(R.string.accounts_notes), Amounts.count(b.spendableOutputs.toLong()))
                    BalanceRow(stringResource(R.string.accounts_bond), stringResource(R.string.accounts_bond_none))
                    // A wallet whose restore height is zero is scanning from
                    // genesis, which looks exactly like having no money for the
                    // day and a half it takes to arrive.
                    if (b.scannedTo in 1..(b.tip - 100_000)) {
                        Spacer(Modifier.height(12.dp))
                        // A slow first scan is routine, not a fault — a wallet
                        // made before a node was reachable, catching up. It
                        // carries its own "Skip ahead", so it reads as a neutral
                        // heads-up, not the red of something broken.
                        Card(colors = CardDefaults.cardColors(
                            containerColor = MaterialTheme.colorScheme.surfaceVariant)) {
                            Column(Modifier.padding(12.dp)) {
                                Text(
                                    stringResource(R.string.accounts_scanning_from_genesis_title),
                                    style = MaterialTheme.typography.labelLarge,
                                )
                                Spacer(Modifier.height(4.dp))
                                Text(
                                    stringResource(
                                        R.string.accounts_scanning_from_genesis_desc,
                                        b.scannedTo, b.tip,
                                    ),
                                    style = MaterialTheme.typography.bodySmall,
                                )
                                Spacer(Modifier.height(10.dp))
                                Button(onClick = { rescanOpen = true }) {
                                    Text(stringResource(R.string.accounts_skip_ahead))
                                }
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
        720L to stringResource(R.string.accounts_about_a_day_ago),
        5_040L to stringResource(R.string.accounts_about_a_week_ago),
        21_600L to stringResource(R.string.accounts_about_a_month_ago),
    )
    var custom by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.accounts_skip_ahead_title)) },
        text = {
            Column {
                Text(
                    stringResource(R.string.accounts_skip_ahead_desc),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(12.dp))
                suggestions.forEach { (back, label) ->
                    val h = (tip - back).coerceAtLeast(0)
                    TextButton(onClick = { onPick(h) }, modifier = Modifier.fillMaxWidth()) {
                        Text(
                            stringResource(R.string.accounts_skip_option, label, h),
                            modifier = Modifier.fillMaxWidth(),
                        )
                    }
                }
                Spacer(Modifier.height(8.dp))
                OutlinedTextField(
                    value = custom,
                    onValueChange = { custom = Amounts.typedNumber(it).filter { c -> c in '0'..'9' } },
                    label = { Text(stringResource(R.string.accounts_block_height_label)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = { custom.toLongOrNull()?.let(onPick) },
                enabled = custom.toLongOrNull()?.let { it in 0..tip } == true,
            ) { Text(stringResource(R.string.accounts_use_that_height)) }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(R.string.accounts_cancel)) }
        },
    )
}
