package org.ducatproject.ducat.ui

import android.content.Context
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.LOCK_BLOCKS
import org.ducatproject.ducat.Ledger
import org.ducatproject.ducat.R
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
        properties = fullScreenDialogProperties(),
    ) {
      Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
        Scaffold(
            topBar = {
                TopAppBar(
                    colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.background,
                ),
                                    title = {
                        Text(
                            if (sent) stringResource(R.string.txdetail_payment_sent)
                            else stringResource(R.string.txdetail_payment_received)
                        )
                    },
                    navigationIcon = {
                        IconButton(onClick = onClose) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.txdetail_back))
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
                    e.pending -> stringResource(R.string.txdetail_status_broadcast)
                    e.unexplained -> stringResource(R.string.txdetail_status_unexplained)
                    confirmations in 1 until LOCK_BLOCKS -> stringResource(
                        R.string.txdetail_confirmations_progress, confirmations, LOCK_BLOCKS,
                    )
                    confirmations > 0 -> pluralStringResource(
                        R.plurals.txdetail_confirmed, confirmations.toInt(), confirmations,
                    )
                    else -> stringResource(R.string.txdetail_not_yet_confirmed)
                },
                style = MaterialTheme.typography.bodySmall,
                color = if (e.pending || confirmations < LOCK_BLOCKS)
                    MaterialTheme.ducat.changePending else MaterialTheme.ducat.settled,
            )

            Section(stringResource(R.string.txdetail_section_payment))
            Field(stringResource(R.string.txdetail_when), whenText(context, e.timestamp))
            Field(
                if (sent) stringResource(R.string.txdetail_to) else stringResource(R.string.txdetail_from),
                counterpartyText(context, e),
            )
            e.address?.let { Field(stringResource(R.string.txdetail_address), it, mono = true, copyable = true) }
            e.note?.takeIf { it.isNotBlank() }?.let { Field(stringResource(R.string.txdetail_note), it) }
            if (sent) {
                Field(stringResource(R.string.txdetail_amount), bothUnits(e.amountPxmr))
                Field(stringResource(R.string.txdetail_network_fee), bothUnits(e.feePxmr))
                if (e.changePxmr > 0) {
                    // Named explicitly because it is the number that makes a
                    // send look like a receipt when it is not labelled.
                    Field(
                        stringResource(R.string.txdetail_change_back), bothUnits(e.changePxmr),
                        note = stringResource(R.string.txdetail_change_back_note),
                    )
                }
                Field(stringResource(R.string.txdetail_total_leaving), bothUnits(-e.netPxmr))
            }
            Field(
                stringResource(R.string.txdetail_balance_after),
                bothUnits(e.balanceAfterPxmr),
                note = if (e.pending) stringResource(R.string.txdetail_balance_after_note) else null,
            )

            // The half a bank statement never has: what the money was *for*.
            // From the receipt store, not the thread — conversations get
            // deleted, a taxi's especially, and the receipt outlives the small
            // talk the way a paper one outlives the ride. A bare transfer
            // shows nothing here, which is itself the argument for paying
            // through DUCAT.
            if (e.receipted) {
                Section(stringResource(R.string.txdetail_section_receipt))
                e.items.forEach { i ->
                    Field(i.description, Amounts.show(context, i.amountPxmr).primary, mono = true)
                }
                e.taxPxmr?.let { Field(stringResource(R.string.txdetail_tax), Amounts.show(context, it).primary, mono = true) }
                Field(stringResource(R.string.txdetail_total_receipted), bothUnits(e.amountPxmr), mono = true)
                Field(
                    stringResource(R.string.txdetail_issued_by),
                    e.receiptBy ?: stringResource(R.string.txdetail_the_payee),
                    note = if (e.receiptAt > 0) whenText(context, e.receiptAt) else null,
                )
            }
            e.contactHex?.let { hex ->
                TextButton(onClick = {
                    org.ducatproject.ducat.MainActivity.openChat.value = hex
                }) { Text(stringResource(R.string.txdetail_open_conversation)) }
            }

            Section(stringResource(R.string.txdetail_section_chain))
            if (e.txid.isNotEmpty()) {
                Field(stringResource(R.string.txdetail_transaction), e.txid, mono = true, copyable = true)
            } else {
                Field(stringResource(R.string.txdetail_transaction), stringResource(R.string.txdetail_not_known),
                    note = stringResource(R.string.txdetail_not_known_note))
            }
            Field(
                stringResource(R.string.txdetail_block),
                if (e.height > 0) Amounts.count(e.height) else stringResource(R.string.txdetail_not_in_block),
            )
            Field(stringResource(R.string.txdetail_confirmations), Amounts.count(confirmations.coerceAtLeast(0)))
            val c = e.chain
            if (c != null) {
                Field(
                    stringResource(R.string.txdetail_format),
                    stringResource(R.string.txdetail_format_ringct, c.version) +
                        if (c.coinbase) stringResource(R.string.txdetail_format_coinbase_suffix) else "",
                )
                Field(stringResource(R.string.txdetail_ring_size), Amounts.count(c.ringSize.toLong()),
                    note = stringResource(R.string.txdetail_ring_size_note, c.ringSize))
                Field(
                    stringResource(R.string.txdetail_inputs_outputs),
                    stringResource(R.string.txdetail_in_out, c.inputCount, c.outputCount),
                )
                Field(stringResource(R.string.txdetail_fee), bothUnits(c.feePxmr))
                Field(
                    stringResource(R.string.txdetail_extra_field),
                    pluralStringResource(R.plurals.txdetail_extra_bytes, c.extraLen, c.extraLen),
                    note = stringResource(R.string.txdetail_extra_field_note))
                if (c.additionalTimelock > 0) {
                    Field(stringResource(R.string.txdetail_extra_timelock), Amounts.count(c.additionalTimelock))
                }
            } else if (!e.pending) {
                Field(
                    stringResource(R.string.txdetail_chain_details), stringResource(R.string.txdetail_not_fetched),
                    note = stringResource(R.string.txdetail_not_fetched_note),
                )
            }

            if (e.ours.isNotEmpty()) {
                Section(
                    if (sent) stringResource(R.string.txdetail_outputs_returned)
                    else stringResource(R.string.txdetail_outputs_received)
                )
                e.ours.forEach { o ->
                    Field(
                        "${formatXmr(o.amountPxmr)} XMR",
                        if (o.spent) stringResource(R.string.txdetail_spent)
                        else stringResource(R.string.txdetail_unspent),
                        note = stringResource(R.string.txdetail_block_n, o.height),
                    )
                    Field(
                        stringResource(R.string.txdetail_key_image),
                        o.keyImage.ifEmpty { stringResource(R.string.txdetail_none) },
                        mono = true, copyable = true,
                    )
                }
            }
            if (e.consumed.isNotEmpty()) {
                Section(stringResource(R.string.txdetail_outputs_this_spent))
                e.consumed.forEach { o ->
                    Field(
                        "${formatXmr(o.amountPxmr)} XMR",
                        stringResource(R.string.txdetail_from_block, o.height),
                    )
                    Field(stringResource(R.string.txdetail_key_image), o.keyImage, mono = true, copyable = true)
                }
            }

            Section(stringResource(R.string.txdetail_section_privacy))
            Text(
                if (sent)
                    stringResource(R.string.txdetail_privacy_sent, e.chain?.ringSize ?: 16)
                else
                    stringResource(R.string.txdetail_privacy_received),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(32.dp))
        }
        }
      }
    }
}

private fun counterpartyText(context: Context, e: Ledger.Event): String = when {
    // Same answer as the row it was opened from — see Activity.who.
    e.escrow != null -> {
        val sent = e.direction == Ledger.Direction.Sent
        val title = e.escrow!!
        when {
            title.isNotBlank() && sent ->
                context.getString(R.string.activity_escrow_in, title)
            title.isNotBlank() ->
                context.getString(R.string.activity_escrow_out, title)
            sent -> context.getString(R.string.activity_escrow_in_untitled)
            else -> context.getString(R.string.activity_escrow_out_untitled)
        }
    }
    e.counterparty != null -> when (e.source) {
        Ledger.Source.Notice -> context.getString(R.string.txdetail_counterparty_chat, e.counterparty)
        else -> e.counterparty
    }
    e.address != null -> context.getString(R.string.txdetail_an_address)
    e.direction == Ledger.Direction.Sent -> context.getString(R.string.txdetail_not_recorded)
    else -> context.getString(R.string.txdetail_unknown_sender)
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

/**
 * Both units on one line, for the figures a person actually reads.
 *
 * A statement's job is to let somebody check that the right amount of money
 * moved, and they cannot do that in a unit they do not price anything in. The
 * piconero stays beside it, because this is also the screen you open when you
 * want the exact chain figure.
 *
 * Not used for the individual outputs further down: those are key images and
 * block heights, where a currency conversion is noise rather than help.
 */
@Composable
private fun bothUnits(pxmr: Long): String {
    val s = Amounts.show(LocalContext.current, pxmr)
    return s.secondary?.let { "${s.primary} \u00b7 $it" } ?: s.primary
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
            .then(if (copyable) Modifier.clickable {
                copyText(context, value, context.getString(R.string.txdetail_copied, label))
            } else Modifier)
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
