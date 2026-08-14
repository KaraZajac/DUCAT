package org.ducatproject.ducat.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.widget.Toast
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.LOCK_BLOCKS
import org.ducatproject.ducat.Ledger
import org.ducatproject.ducat.formatXmr

/**
 * One transaction, in full.
 *
 * Everything a block explorer would show for it, plus the two things an
 * explorer cannot know: which of these outputs are ours, and who the other
 * party was. It is deliberately exhaustive — this is the screen someone opens
 * when a number looks wrong, and a summary is exactly what does not help then.
 *
 * What it will not do is guess. Fields the wallet has not fetched say so rather
 * than showing a plausible zero.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TxDetailScreen(e: Ledger.Event, tip: Long, onClose: () -> Unit) {
    val context = LocalContext.current
    BackHandler(onBack = onClose)
    val sent = e.direction == Ledger.Direction.Sent
    val confirmations = if (e.height > 0 && tip >= e.height) tip - e.height + 1 else 0

    // Its own window rather than content inside the Activity tab: nesting a
    // Scaffold inside the app's Scaffold stacks two top bars and leaves the
    // bottom navigation over a screen that is not a tab.
    Dialog(
        onDismissRequest = onClose,
        properties = DialogProperties(
            usePlatformDefaultWidth = false,
            dismissOnBackPress = true,
            decorFitsSystemWindows = false,
        ),
    ) {
      Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
        Scaffold(
            topBar = {
                TopAppBar(
                    colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.background,
                ),
                                    title = { Text(if (sent) "Payment sent" else "Payment received") },
                    navigationIcon = {
                        IconButton(onClick = onClose) {
                            Icon(Icons.Filled.ArrowBack, contentDescription = "Back")
                        }
                    },
                )
            },
        ) { padding ->
        Column(
            Modifier.padding(padding).fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(16.dp),
        ) {
            val amount = Amounts.show(context, e.amountPxmr)
            Text(
                "${if (sent) "−" else "+"}${amount.primary}",
                style = MaterialTheme.typography.displayMedium,
            )
            amount.secondary?.let {
                Text(it, style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Spacer(Modifier.height(4.dp))
            Text(
                when {
                    e.pending -> "Broadcast, not yet in a block"
                    e.unexplained -> "This output was spent and the transaction " +
                        "is not one we can identify"
                    confirmations in 1 until LOCK_BLOCKS ->
                        "$confirmations of $LOCK_BLOCKS confirmations"
                    confirmations > 0 -> "Confirmed · $confirmations confirmations"
                    else -> "Not yet confirmed"
                },
                style = MaterialTheme.typography.bodySmall,
                color = if (e.pending || confirmations < LOCK_BLOCKS)
                    MaterialTheme.ducat.changePending else MaterialTheme.ducat.settled,
            )

            Section("Payment")
            Field("When", whenText(e.timestamp))
            Field(if (sent) "To" else "From", counterpartyText(e))
            e.address?.let { Field("Address", it, mono = true, copyable = true) }
            e.note?.takeIf { it.isNotBlank() }?.let { Field("Note", it) }
            if (sent) {
                Field("Amount", "${formatXmr(e.amountPxmr)} XMR")
                Field("Network fee", "${formatXmr(e.feePxmr)} XMR")
                if (e.changePxmr > 0) {
                    // Named explicitly because it is the number that makes a
                    // send look like a receipt when it is not labelled.
                    Field(
                        "Change back to you", "${formatXmr(e.changePxmr)} XMR",
                        note = "Monero spends whole outputs, so the remainder " +
                            "returns to your own wallet as a new one.",
                    )
                }
                Field("Total leaving the wallet", "${formatXmr(-e.netPxmr)} XMR")
            }
            Field(
                "Balance after this",
                "${formatXmr(e.balanceAfterPxmr)} XMR",
                note = if (e.pending) "Unchanged until the spend is seen on chain." else null,
            )

            // The half a bank statement never has: what the money was *for*.
            // From the receipt store, not the thread — conversations get
            // deleted, a taxi's especially, and the receipt outlives the small
            // talk the way a paper one outlives the ride. A bare transfer
            // shows nothing here, which is itself the argument for paying
            // through DUCAT.
            if (e.receipted) {
                Section("Receipt")
                e.items.forEach { i ->
                    Field(i.description, "${formatXmr(i.amountPxmr)} XMR", mono = true)
                }
                e.taxPxmr?.let { Field("Tax", "${formatXmr(it)} XMR", mono = true) }
                Field("Total receipted", "${formatXmr(e.amountPxmr)} XMR", mono = true)
                Field(
                    "Issued by",
                    e.receiptBy ?: "the payee",
                    note = if (e.receiptAt > 0) whenText(e.receiptAt) else null,
                )
            }
            e.contactHex?.let { hex ->
                TextButton(onClick = {
                    org.ducatproject.ducat.MainActivity.openChat.value = hex
                }) { Text("Open the conversation") }
            }

            Section("On the chain")
            if (e.txid.isNotEmpty()) {
                Field("Transaction", e.txid, mono = true, copyable = true)
            } else {
                Field("Transaction", "not known",
                    note = "Recorded before the wallet kept transaction ids.")
            }
            Field("Block", if (e.height > 0) "${e.height}" else "not yet in a block")
            Field("Confirmations", if (confirmations > 0) "$confirmations" else "0")
            val c = e.chain
            if (c != null) {
                Field("Format", "RingCT v${c.version}${if (c.coinbase) " · coinbase" else ""}")
                Field("Ring size", "${c.ringSize}",
                    note = "Decoys plus the real spend. Nobody reading the chain " +
                        "can tell which of the ${c.ringSize} was actually spent.")
                Field("Inputs / outputs", "${c.inputCount} in · ${c.outputCount} out")
                Field("Fee", "${formatXmr(c.feePxmr)} XMR")
                Field("Extra field", "${c.extraLen} bytes",
                    note = "Carries the transaction public key, and a payment id if one was used.")
                if (c.additionalTimelock > 0) {
                    Field("Extra timelock", "${c.additionalTimelock}")
                }
            } else if (!e.pending) {
                Field(
                    "Chain details", "not fetched yet",
                    note = "The wallet reads these a few at a time in the " +
                        "background. Leave the app open for a moment.",
                )
            }

            if (e.ours.isNotEmpty()) {
                Section(if (sent) "Outputs returned to you" else "Outputs you received")
                e.ours.forEach { o ->
                    Field(
                        "${formatXmr(o.amountPxmr)} XMR",
                        if (o.spent) "spent" else "unspent",
                        note = "block ${o.height}",
                    )
                    Field("  Key image", o.keyImage.ifEmpty { "none" }, mono = true, copyable = true)
                }
            }
            if (e.consumed.isNotEmpty()) {
                Section("Outputs this spent")
                e.consumed.forEach { o ->
                    Field("${formatXmr(o.amountPxmr)} XMR", "from block ${o.height}")
                    Field("  Key image", o.keyImage, mono = true, copyable = true)
                }
            }

            Section("Privacy")
            Text(
                if (sent)
                    "The recipient is not on the chain. What is published is a " +
                        "one-time key nobody can link to their address, and a ring " +
                        "of ${e.chain?.ringSize ?: 16} possible sources for each input."
                else
                    "Monero does not record who sent this. A name appears here only " +
                        "when a contact told us in the conversation, and even then it " +
                        "is their claim — the money is verified, the sender is not.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(32.dp))
        }
        }
      }
    }
}

private fun counterpartyText(e: Ledger.Event): String = when {
    e.counterparty != null -> when (e.source) {
        Ledger.Source.Notice -> "${e.counterparty} (they said so in chat)"
        else -> e.counterparty
    }
    e.address != null -> "an address"
    e.direction == Ledger.Direction.Sent -> "not recorded"
    else -> "unknown — Monero does not carry a sender"
}

@Composable
private fun Section(title: String) {
    Spacer(Modifier.height(22.dp))
    Text(
        title,
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.primary,
    )
    Spacer(Modifier.height(4.dp))
    HorizontalDivider()
    Spacer(Modifier.height(6.dp))
}

@Composable
private fun Field(
    label: String,
    value: String,
    mono: Boolean = false,
    copyable: Boolean = false,
    note: String? = null,
) {
    val context = LocalContext.current
    Column(
        Modifier.fillMaxWidth()
            .then(if (copyable) Modifier.clickable { copy(context, label, value) } else Modifier)
            .padding(vertical = 6.dp),
    ) {
        Row(verticalAlignment = Alignment.Top) {
            Text(
                label,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.weight(0.42f),
            )
            Text(
                value,
                style = if (mono) MaterialTheme.typography.labelSmall
                else MaterialTheme.typography.bodySmall,
                fontFamily = if (mono) FontFamily.Monospace else null,
                modifier = Modifier.weight(0.58f),
            )
        }
        note?.let {
            Text(
                it,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.outline,
                modifier = Modifier.padding(top = 2.dp),
            )
        }
    }
}

private fun copy(context: Context, label: String, value: String) {
    val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    cm.setPrimaryClip(ClipData.newPlainText(label, value))
    Toast.makeText(context, "$label copied", Toast.LENGTH_SHORT).show()
}
