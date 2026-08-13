package org.ducatproject.ducat.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.*
import java.math.BigDecimal

/**
 * Who to pay, then how much.
 *
 * Two steps rather than one form, because the two questions have different
 * answers available: a contact is chosen from a list, an address is scanned or
 * pasted, and only once there is a destination does an amount mean anything.
 *
 * **A contact is the preferred path and is listed first.** Paying an address is
 * a plain Monero transfer: no name, no note, no receipt, and nothing tying it
 * to a conversation. Paying a contact goes through §16.13, so the payment sits
 * in a thread with a note attached and the other side can ask for it in the
 * first place. Both work; only one of them is DUCAT.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PaySheet(
    prefillAddress: String? = null,
    prefillAmountPxmr: Long = 0,
    prefillContact: Contact? = null,
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    var target by remember {
        mutableStateOf<PayTarget?>(
            when {
                prefillContact != null -> PayTarget.ToContact(prefillContact)
                prefillAddress != null -> PayTarget.ToAddress(prefillAddress)
                else -> null
            }
        )
    }
    var scanning by remember { mutableStateOf(false) }

    if (scanning) {
        QrScanner(
            prompt = "A Monero address, a monero: code, or a DUCAT card",
            onResult = { raw ->
                scanning = false
                target = readScan(context, raw)
            },
            onDismiss = { scanning = false },
        )
        return
    }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        when (val t = target) {
            null -> ChooseTarget(
                onPick = { target = it },
                onScan = { scanning = true },
            )
            else -> AmountStep(
                target = t,
                prefillAmountPxmr = prefillAmountPxmr,
                onBack = { if (prefillAddress == null && prefillContact == null) target = null },
                onDone = onDismiss,
            )
        }
    }
}

/** Where money is going. The two are not interchangeable — see [PaySheet]. */
sealed interface PayTarget {
    data class ToContact(val contact: Contact) : PayTarget
    data class ToAddress(val address: String) : PayTarget
}

/**
 * Read whatever the camera found.
 *
 * A DUCAT card is recognised but **not** silently turned into a payment target:
 * a card is an introduction, and someone scanning one at a payment screen meant
 * to add a person, not to pay a stranger their card happens to name.
 */
private fun readScan(context: android.content.Context, raw: String): PayTarget? {
    val t = raw.trim()
    if (t.startsWith("ducat:card/")) {
        // Match it against contacts we already have; anything else needs the
        // add-contact flow, which shows who it is before committing.
        val known = runCatching { uniffi.ducat_mobile.readContactCard(t) }.getOrNull()
        val hex = known?.persona?.joinToString("") { "%02x".format(it) }
        val c = hex?.let { h -> ContactStore(context).all().firstOrNull { it.personaHex == h } }
        return c?.let { PayTarget.ToContact(it) }
    }
    val addr = t.removePrefix("monero:").substringBefore("?")
    return if (addr.length in 90..110) PayTarget.ToAddress(addr) else null
}

@Composable
private fun ChooseTarget(onPick: (PayTarget) -> Unit, onScan: () -> Unit) {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val contacts = remember(version) { ContactStore(context).all() }
    var address by remember { mutableStateOf("") }

    Column(Modifier.padding(horizontal = 20.dp).padding(bottom = 24.dp)) {
        Text("Send or request", style = MaterialTheme.typography.titleLarge)
        Spacer(Modifier.height(12.dp))

        OutlinedButton(onClick = onScan, modifier = Modifier.fillMaxWidth()) {
            Icon(Icons.Filled.QrCodeScanner, null, Modifier.size(18.dp))
            Spacer(Modifier.width(8.dp))
            Text("Scan a code")
        }

        if (contacts.isNotEmpty()) {
            Spacer(Modifier.height(16.dp))
            Text("Your contacts", style = MaterialTheme.typography.labelLarge)
            Text(
                "A payment to a contact carries a note and lands in your chat. " +
                    "An address does not.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(4.dp))
            LazyColumn(Modifier.heightIn(max = 240.dp)) {
                items(contacts, key = { it.personaHex }) { c ->
                    ListItem(
                        headlineContent = { Text(c.displayName()) },
                        supportingContent = {
                            Text(
                                c.personaHex.take(16) + "…",
                                fontFamily = FontFamily.Monospace,
                                style = MaterialTheme.typography.bodySmall,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        },
                        leadingContent = { Avatar(c.displayName()) },
                        modifier = Modifier.clickable { onPick(PayTarget.ToContact(c)) },
                    )
                }
            }
        }

        Spacer(Modifier.height(16.dp))
        Text("Or a Monero address", style = MaterialTheme.typography.labelLarge)
        Spacer(Modifier.height(6.dp))
        OutlinedTextField(
            value = address,
            onValueChange = { address = it },
            placeholder = { Text("5… or 7…") },
            modifier = Modifier.fillMaxWidth(),
            minLines = 2,
        )
        Spacer(Modifier.height(8.dp))
        Button(
            onClick = { onPick(PayTarget.ToAddress(address.trim())) },
            enabled = address.trim().length in 90..110,
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Continue") }
        if (address.isNotBlank() && address.trim().length !in 90..110) {
            Text(
                "That does not look like a Monero address.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
            )
        }
    }
}

/**
 * How much, in whichever unit the user is thinking in.
 *
 * The entry field is the unit they picked; the other one sits underneath and
 * updates as they type. Converting in your head to check a payment is exactly
 * the moment a mistake costs money.
 */
@Composable
private fun AmountStep(
    target: PayTarget,
    prefillAmountPxmr: Long,
    onBack: () -> Unit,
    onDone: () -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val version by ContactStore.changes.collectAsState()
    val b = remember(version) { Wallet.balances(context) }
    var fiatEntry by remember { mutableStateOf(Amounts.preferFiat(context)) }
    val rate = remember(version) { RateStore(context).cached()?.first }
    val cur = remember { Amounts.currency(context) }

    var typed by remember {
        mutableStateOf(if (prefillAmountPxmr > 0) formatXmr(prefillAmountPxmr) else "")
    }
    var note by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var done by remember { mutableStateOf<String?>(null) }
    var confirming by remember { mutableStateOf(false) }

    // What was typed, as piconero. Entering in a currency converts at the rate
    // shown; entering in XMR does not convert at all.
    val pxmr = remember(typed, fiatEntry, rate) {
        val v = typed.trim().toBigDecimalOrNull() ?: return@remember null
        val xmr = if (fiatEntry && rate != null && rate > 0) {
            v.divide(BigDecimal(rate), 12, java.math.RoundingMode.DOWN)
        } else v
        xmr.multiply(BigDecimal(1_000_000_000_000L)).toLong().takeIf { it > 0 }
    }

    Column(Modifier.padding(horizontal = 20.dp).padding(bottom = 24.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) { Icon(Icons.Filled.ArrowBack, "Back") }
            Spacer(Modifier.width(4.dp))
            when (target) {
                is PayTarget.ToContact -> {
                    Avatar(target.contact.displayName())
                    Spacer(Modifier.width(10.dp))
                    Text(target.contact.displayName(),
                         style = MaterialTheme.typography.titleMedium)
                }
                is PayTarget.ToAddress -> Text(
                    target.address.take(12) + "…" + target.address.takeLast(6),
                    fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }

        Spacer(Modifier.height(20.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(
                value = typed,
                onValueChange = { typed = it.filter { c -> c.isDigit() || c == '.' } },
                placeholder = { Text("0") },
                textStyle = MaterialTheme.typography.headlineMedium,
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                modifier = Modifier.weight(1f),
            )
            Spacer(Modifier.width(10.dp))
            if (rate != null) {
                // Switching the unit keeps the number and re-reads it, rather
                // than converting what was typed: someone who meant "20" meant
                // twenty of whatever they just chose.
                AssistChip(
                    onClick = { fiatEntry = !fiatEntry },
                    label = { Text(if (fiatEntry) cur else "XMR") },
                    trailingIcon = { Icon(Icons.Filled.SwapVert, null, Modifier.size(16.dp)) },
                )
            } else {
                Text("XMR", style = MaterialTheme.typography.labelLarge)
            }
        }
        pxmr?.let {
            Text(
                if (fiatEntry) "${formatXmr(it)} XMR"
                else Amounts.show(context, it).let { s -> s.secondary ?: "" },
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        Spacer(Modifier.height(12.dp))
        Text(
            "Spendable: ${Amounts.show(context, b.spendablePxmr).primary}",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        if (target is PayTarget.ToContact) {
            Spacer(Modifier.height(12.dp))
            OutlinedTextField(
                value = note,
                onValueChange = { if (it.length <= 128) note = it },
                label = { Text("What for") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
        }

        Spacer(Modifier.height(16.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            if (target is PayTarget.ToContact) {
                OutlinedButton(
                    onClick = {
                        val amt = pxmr ?: return@OutlinedButton
                        busy = true; error = null
                        scope.launch {
                            val r = withContext(Dispatchers.IO) {
                                runCatching {
                                    Mailbox.send(
                                        context, target.contact,
                                        note.ifBlank { "Payment request" },
                                        PersonaStore(context).personaHex(),
                                        kind = 1, amountPxmr = amt,
                                        payto = WalletStore(context).address(),
                                    )
                                }
                            }
                            busy = false
                            r.onSuccess { done = "Request sent" }
                                .onFailure { error = it.message ?: "could not send" }
                        }
                    },
                    enabled = !busy && pxmr != null,
                    modifier = Modifier.weight(1f),
                ) { Text("Request") }
            }
            val payable = target !is PayTarget.ToContact ||
                target.contact.theirAddress != null || prefillAmountPxmr > 0
            Button(
                onClick = { confirming = true },
                enabled = !busy && pxmr != null && payable &&
                    b.spendablePxmr >= (pxmr ?: 0),
                modifier = Modifier.weight(1f),
            ) {
                if (busy) CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                else Text("Send")
            }
        }

        if (target is PayTarget.ToContact && target.contact.theirAddress == null &&
            prefillAmountPxmr == 0L
        ) {
            Spacer(Modifier.height(8.dp))
            Text(
                "${target.contact.displayName()} has not shared an address, so you can " +
                    "ask but not send yet. A request carries one back.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        done?.let {
            Spacer(Modifier.height(12.dp))
            Text(it, color = MaterialTheme.ducat.settled)
        }
        error?.let {
            Spacer(Modifier.height(12.dp))
            Text(it, color = MaterialTheme.colorScheme.error,
                 style = MaterialTheme.typography.bodySmall)
        }
    }

    if (confirming && pxmr != null) {
        val dest = when (target) {
            is PayTarget.ToContact -> target.contact.theirAddress
            is PayTarget.ToAddress -> target.address
        }
        ConfirmSend(
            pxmr = pxmr,
            destination = dest,
            contactName = (target as? PayTarget.ToContact)?.contact?.displayName(),
            onCancel = { confirming = false },
            onConfirm = {
                confirming = false
                busy = true; error = null
                scope.launch {
                    val r = withContext(Dispatchers.IO) {
                        runCatching {
                            val node = NodeStore(context).lastGood()
                                ?: throw IllegalStateException("no node — check Status")
                            // A contact payment still needs an address, and one
                            // only exists if they asked. §16.13 keeps addresses
                            // out of the contact record on purpose.
                            // Their published address if they chose to publish
                            // one, otherwise the one from a request. Neither
                            // exists until they have offered it (§16.12).
                            val to = dest
                                ?: (target as? PayTarget.ToContact)?.contact?.theirAddress
                                ?: throw IllegalStateException(
                                    "They have not shared an address. Ask them to send " +
                                        "a request, or to turn on \"let contacts pay me " +
                                        "directly\"."
                                )
                            Wallet.send(context, node, to, pxmr)
                        }
                    }
                    busy = false
                    r.onSuccess { done = "Sent · ${it.txidHex.take(16)}…" }
                        .onFailure { error = it.message ?: "could not send" }
                }
            },
        )
    }
}

/** §15.5's checkpoint, which nothing may shorten the path to. */
@Composable
private fun ConfirmSend(
    pxmr: Long,
    destination: String?,
    contactName: String?,
    onCancel: () -> Unit,
    onConfirm: () -> Unit,
) {
    val context = LocalContext.current
    val a = Amounts.show(context, pxmr)
    AlertDialog(
        onDismissRequest = onCancel,
        title = {
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
                contactName?.let { Text("To $it", style = MaterialTheme.typography.bodyMedium) }
                destination?.let {
                    Spacer(Modifier.height(6.dp))
                    Text(it, fontFamily = FontFamily.Monospace,
                         style = MaterialTheme.typography.bodySmall)
                }
                Spacer(Modifier.height(10.dp))
                Text(
                    "Monero payments cannot be reversed or cancelled. Check the " +
                        "address — there is nobody to appeal to if it is wrong.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.ducat.changePending,
                )
            }
        },
        confirmButton = { TextButton(onClick = onConfirm) { Text("Send") } },
        dismissButton = { TextButton(onClick = onCancel) { Text("Cancel") } },
    )
}
