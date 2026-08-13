package org.ducatproject.ducat.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.*
import org.ducatproject.ducat.DucatLog
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

    // A whole screen, not a sheet. A bottom sheet has to share the window with
    // the keyboard, and on a payment screen the thing it covers is the amount
    // and the button under it. `decorFitsSystemWindows = false` is what lets
    // imePadding do its job inside a dialog.
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(
            usePlatformDefaultWidth = false,
            dismissOnBackPress = true,
            decorFitsSystemWindows = false,
        ),
    ) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            when (val t = target) {
                null -> ChooseTarget(
                    onPick = { target = it },
                    onScan = { scanning = true },
                    onClose = onDismiss,
                )
                else -> AmountStep(
                    target = t,
                    prefillAmountPxmr = prefillAmountPxmr,
                    // With a target chosen for us there is no earlier step to
                    // return to, so back leaves rather than doing nothing.
                    onBack = {
                        if (prefillAddress == null && prefillContact == null) target = null
                        else onDismiss()
                    },
                    onDone = onDismiss,
                )
            }
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
private fun ChooseTarget(
    onPick: (PayTarget) -> Unit,
    onScan: () -> Unit,
    onClose: () -> Unit,
) {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val contacts = remember(version) { ContactStore(context).all() }
    var address by remember { mutableStateOf("") }

    Column(
        Modifier
            .fillMaxSize()
            .imePadding()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp)
            .padding(bottom = 24.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(top = 12.dp)) {
            IconButton(onClick = onClose) { Icon(Icons.Filled.Close, "Close") }
            Spacer(Modifier.width(4.dp))
            Text("Send or request", style = MaterialTheme.typography.titleLarge)
        }
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
    var priority by remember { mutableIntStateOf(1) }

    var typed by remember {
        mutableStateOf(if (prefillAmountPxmr > 0) formatXmr(prefillAmountPxmr) else "")
    }
    var note by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var done by remember { mutableStateOf<String?>(null) }
    /**
     * Asking for money, or sending it.
     *
     * One screen was doing both, and the send half owns most of it: a maximum,
     * a fee estimate, a "left after", a speed. None of that is true of a
     * request — you are asking someone else to spend, so your balance and your
     * fee are not part of the question. Typing an amount therefore priced a
     * transaction the user had not asked for, and the way to ask was a second
     * button beside the one everything on screen had been describing.
     *
     * The toggle makes the mode the first decision rather than the last, and
     * everything that only applies to a spend disappears when it does not.
     */
    var asking by remember { mutableStateOf(false) }
    // Asking needs a thread to ask in. A bare address has no way back.
    val canAsk = target is PayTarget.ToContact
    var confirming by remember { mutableStateOf(false) }
    val amountFocus = remember { FocusRequester() }

    LaunchedEffect(Unit) {
        kotlinx.coroutines.delay(150)
        runCatching { amountFocus.requestFocus() }
    }

    // A finished payment has nothing left to do here, and staying put is worse
    // than idle: the send consumed the note it was spending, so Max drops to
    // zero and the form reads as an error the moment it succeeds. Long enough
    // to read the confirmation, then back to wherever this was opened from —
    // the conversation, or Home. `onDone` existed and was never called, which
    // is why it never left.
    LaunchedEffect(done) {
        if (done != null) {
            kotlinx.coroutines.delay(1600)
            onDone()
        }
    }

    // The ceiling, priced. Not the balance: offering the balance as the maximum
    // is how a wallet lets someone type a number it will then refuse, after
    // they have already decided.
    var maxPxmr by remember { mutableStateOf(0L) }
    // Sticky: once someone has asked for the maximum, changing the speed
    // changes the fee, and the amount has to follow or the screen is showing a
    // number that is no longer the maximum it just promised.
    var maxLocked by remember { mutableStateOf(false) }
    LaunchedEffect(version, priority) {
        maxPxmr = withContext(Dispatchers.IO) { Wallet.maxSendable(context, priority) }
    }
    LaunchedEffect(maxPxmr, maxLocked, fiatEntry) {
        if (maxLocked && maxPxmr > 0) {
            typed = if (fiatEntry && rate != null) "%.2f".format(maxPxmr / 1e12 * rate)
                    else formatXmr(maxPxmr)
        }
    }

    val pxmr = remember(typed, fiatEntry, rate) {
        val v = typed.trim().toBigDecimalOrNull() ?: return@remember null
        val xmr = if (fiatEntry && rate != null && rate > 0) {
            v.divide(BigDecimal(rate), 12, java.math.RoundingMode.DOWN)
        } else v
        xmr.multiply(BigDecimal(1_000_000_000_000L)).toLong().takeIf { it > 0 }
    }

    var quote by remember { mutableStateOf<Quote?>(null) }
    LaunchedEffect(pxmr, priority, version) {
        val amt = pxmr
        quote = if (amt == null) null
        else withContext(Dispatchers.IO) { Wallet.quote(context, amt, priority) }
    }

    val overMax = pxmr != null && maxPxmr > 0 && pxmr > maxPxmr

    Column(
        Modifier
            .fillMaxSize()
            // The keyboard gets its own room and the rest scrolls, so the
            // buttons never end up underneath it — which is the one moment the
            // screen exists for.
            .imePadding()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp)
            .padding(bottom = 24.dp),
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.padding(top = 12.dp),
        ) {
            IconButton(onClick = onBack) { Icon(Icons.Filled.ArrowBack, "Back") }
            Spacer(Modifier.width(4.dp))
            when (target) {
                is PayTarget.ToContact -> {
                    Avatar(target.contact.displayName())
                    Spacer(Modifier.width(10.dp))
                    Column {
                        Text(target.contact.displayName(),
                             style = MaterialTheme.typography.titleMedium)
                        Text(
                            "in DUCAT",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.outline,
                        )
                    }
                }
                is PayTarget.ToAddress -> Column {
                    Text("Monero address", style = MaterialTheme.typography.titleSmall)
                    Text(
                        target.address.take(10) + "…" + target.address.takeLast(6),
                        fontFamily = FontFamily.Monospace,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.outline,
                    )
                }
            }
        }

        Spacer(Modifier.height(28.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(
                value = typed,
                onValueChange = {
                    typed = it.filter { c -> c.isDigit() || c == '.' }
                    maxLocked = false
                },
                placeholder = { Text("0") },
                textStyle = MaterialTheme.typography.headlineLarge,
                singleLine = true,
                isError = overMax,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                modifier = Modifier.weight(1f).focusRequester(amountFocus),
            )
            Spacer(Modifier.width(10.dp))
            if (rate != null) {
                AssistChip(
                    onClick = {
                    // Keep a locked maximum across the switch — it is the same
                    // amount in a different unit, not a new intention.
                    val wasMax = maxLocked
                    fiatEntry = !fiatEntry
                    typed = ""
                    maxLocked = wasMax
                },
                    label = { Text(if (fiatEntry) cur else "XMR") },
                    trailingIcon = { Icon(Icons.Filled.SwapVert, null, Modifier.size(16.dp)) },
                )
            } else {
                Text("XMR", style = MaterialTheme.typography.labelLarge)
            }
        }

        // The other unit, live, so nobody converts in their head to check.
        pxmr?.let {
            Text(
                if (fiatEntry) "${formatXmr(it)} XMR"
                else Amounts.show(context, it).secondary.orEmpty(),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        Spacer(Modifier.height(12.dp))
        if (asking) {
            Text(
                "What you are asking ${
                    (target as? PayTarget.ToContact)?.contact?.displayName() ?: "them"
                } for. Your balance and the network fee are theirs to worry " +
                    "about, not yours — this is a message, not a transaction.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        } else {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                "Up to ${inUnit(context, maxPxmr, fiatEntry, rate, cur)} after fees",
                style = MaterialTheme.typography.bodySmall,
                color = if (overMax) MaterialTheme.colorScheme.error
                        else MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.weight(1f))
            TextButton(
                onClick = { maxLocked = true },
                enabled = maxPxmr > 0,
            ) { Text(if (maxLocked) "Max ✓" else "Max") }
        }
        if (overMax) {
            Text(
                "That is more than you can send once the fee is counted.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
            )
        }

        // The breakdown, once there is something to break down.
        quote?.let { q ->
            Spacer(Modifier.height(14.dp))
            Card(colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surfaceVariant)) {
                Column(Modifier.padding(14.dp)) {
                    CostRow("Amount", Amounts.show(context, q.amountPxmr).primary)
                    CostRow(
                        "Network fee (estimated)",
                        Amounts.show(context, q.feePxmr).primary,
                    )
                    HorizontalDivider(Modifier.padding(vertical = 8.dp))
                    CostRow("Total", Amounts.show(context, q.totalPxmr).primary, bold = true)
                    CostRow("Left after", Amounts.show(context, q.remainingPxmr).primary)
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "Uses ${q.notes} note(s) · usually confirmed in about " +
                            "${q.minutesToConfirm} minutes",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Text(
                        "The fee is an estimate until the transaction is built; the " +
                            "exact one is shown after sending.",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.outline,
                    )
                }
            }

            Spacer(Modifier.height(10.dp))
            Text("Speed", style = MaterialTheme.typography.labelLarge)
            Spacer(Modifier.height(4.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                listOf("Slow", "Normal", "Fast", "Fastest").forEachIndexed { i, label ->
                    FilterChip(
                        selected = priority == i,
                        onClick = { priority = i },
                        label = { Text(label, style = MaterialTheme.typography.labelSmall) },
                    )
                }
            }
        }
        } // end of the send-only breakdown

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
        // The mode, where PayPal puts it: a pill above the action, so the two
        // verbs are one visible choice rather than two competing buttons.
        if (canAsk) {
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                SegmentedButton(
                    selected = !asking,
                    onClick = { asking = false },
                    shape = SegmentedButtonDefaults.itemShape(0, 2),
                    modifier = Modifier.weight(1f),
                ) { Text("Send", maxLines = 1, softWrap = false) }
                SegmentedButton(
                    selected = asking,
                    onClick = { asking = true },
                    shape = SegmentedButtonDefaults.itemShape(1, 2),
                    modifier = Modifier.weight(1f),
                ) { Text("Request", maxLines = 1, softWrap = false) }
            }
            Spacer(Modifier.height(12.dp))
        }

        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            if (asking && target is PayTarget.ToContact) {
                Button(
                    onClick = {
                        val amt = pxmr ?: return@Button
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
                    // Asking for more than you hold is perfectly reasonable, so
                    // a request is never blocked by the balance.
                    enabled = !busy && pxmr != null,
                    modifier = Modifier.weight(1f).height(52.dp),
                ) {
                    if (busy) CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                    else Text("Request")
                }
            } else {
                val payable = target !is PayTarget.ToContact ||
                    target.contact.theirAddress != null || prefillAmountPxmr > 0
                Button(
                    onClick = { confirming = true },
                    enabled = !busy && pxmr != null && payable && !overMax &&
                        quote?.affordable == true,
                    modifier = Modifier.weight(1f).height(52.dp),
                ) {
                    if (busy) CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                    else Text("Send")
                }
            }
        }

        if (!asking && target is PayTarget.ToContact && target.contact.theirAddress == null &&
            prefillAmountPxmr == 0L
        ) {
            Spacer(Modifier.height(8.dp))
            Text(
                "${target.contact.displayName()} has not shared an address, so you can " +
                    "ask but not send yet. Switch to Request — it carries one back.",
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
            quote = quote,
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
                            val to = dest
                                ?: throw IllegalStateException(
                                    "They have not shared an address. Ask them to send " +
                                        "a request, or to turn on \"let contacts pay me " +
                                        "directly\"."
                                )
                            val contact = (target as? PayTarget.ToContact)?.contact
                            val res = Wallet.send(
                                context, node, to, pxmr,
                                contactHex = contact?.personaHex,
                                note = note.ifBlank { null },
                                priority = priority,
                            )
                            // Tell them, in the thread. §16.13's notice is
                            // advisory — they verify by finding the output — but
                            // without it a payment lands with no explanation and
                            // neither side has a record of what it was for.
                            contact?.let { c ->
                                runCatching {
                                    Mailbox.send(
                                        context, c,
                                        note.ifBlank { "Payment" },
                                        PersonaStore(context).personaHex(),
                                        kind = 2, amountPxmr = pxmr,
                                        // Names the transaction, which is what
                                        // lets their wallet put our name on the
                                        // output when it arrives. Monero carries
                                        // no sender; this is the only channel.
                                        txidHex = res.txidHex,
                                    )
                                }.onFailure {
                                    DucatLog.w(
                                        "Pay",
                                        "sent, but could not tell them: ${it.message}",
                                    )
                                }
                            }
                            res
                        }
                    }
                    busy = false
                    r.onSuccess {
                        done = "Sent · fee ${formatXmr(it.feePxmr.toLong())} XMR · " +
                            "${it.txidHex.take(16)}…"
                    }.onFailure { error = it.message ?: "could not send" }
                }
            },
        )
    }
}

/** An amount in whichever unit the entry field is currently using. */
private fun inUnit(
    context: android.content.Context,
    pxmr: Long,
    fiat: Boolean,
    rate: Double?,
    cur: String,
): String = if (fiat && rate != null) {
    "%s %,.2f".format(cur, pxmr / 1e12 * rate)
} else "${formatXmr(pxmr)} XMR"

@Composable
private fun CostRow(label: String, value: String, bold: Boolean = false) {
    Row(Modifier.fillMaxWidth().padding(vertical = 2.dp)) {
        Text(
            label,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.weight(1f))
        Text(
            value,
            style = if (bold) MaterialTheme.typography.titleSmall
                    else MaterialTheme.typography.bodySmall,
        )
    }
}

/** §15.5's checkpoint, which nothing may shorten the path to. */
@Composable
private fun ConfirmSend(
    pxmr: Long,
    quote: Quote?,
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
                quote?.let { q ->
                    Spacer(Modifier.height(10.dp))
                    Text(
                        "Plus about ${Amounts.show(context, q.feePxmr).primary} in fees — " +
                            "${Amounts.show(context, q.totalPxmr).primary} in total.",
                        style = MaterialTheme.typography.bodySmall,
                    )
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
