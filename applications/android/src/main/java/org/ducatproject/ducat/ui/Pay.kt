package org.ducatproject.ducat.ui

import androidx.activity.compose.BackHandler
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
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
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
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.R
import org.ducatproject.ducat.saidWhy
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
    // The destination survives recreation and process death: a Contact does
    // not fit a Bundle, its persona hex does, and the store re-resolves it —
    // fresher than a snapshot would be. A vanished contact (or no saved
    // target) falls back to the init value, which is the chooser.
    var target by rememberSaveable(
        stateSaver = Saver<PayTarget?, String>(
            save = {
                when (it) {
                    is PayTarget.ToContact -> "c:${it.contact.personaHex}"
                    is PayTarget.ToAddress -> "a:${it.address}"
                    null -> ""
                }
            },
            restore = { s ->
                when {
                    s.startsWith("c:") -> ContactStore(context).all()
                        .firstOrNull { it.personaHex == s.removePrefix("c:") }
                        ?.let { PayTarget.ToContact(it) }
                    s.startsWith("a:") -> PayTarget.ToAddress(s.removePrefix("a:"))
                    else -> null
                }
            },
        ),
    ) {
        mutableStateOf<PayTarget?>(
            when {
                prefillContact != null -> PayTarget.ToContact(prefillContact)
                prefillAddress != null -> PayTarget.ToAddress(prefillAddress)
                else -> null
            }
        )
    }
    var scanning by remember { mutableStateOf(false) }
    // What a code scanned *here* asked for. The sheet's own parameter covers a
    // scan made from the codes screen; this covers the scanner inside it, and
    // without it the amount survived one route into this screen and not the
    // other.
    var scannedPxmr by remember { mutableStateOf(0L) }

    if (scanning) {
        QrScanner(
            prompt = stringResource(R.string.pay_scan_prompt),
            onResult = { raw ->
                scanning = false
                target = readScan(context, raw)
                scannedPxmr = moneroUri(raw)?.second ?: 0L
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
        // dismissOnBackPress off, because the dialog's own handling drops
        // the whole sheet from any step; back is handled inside, where it
        // can step.
        properties = fullScreenDialogProperties(dismissOnBackPress = false),
    ) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            // The system back mirrors the on-screen arrow: a step back, not a
            // bigger Close. Only the first step — or a flow that started with
            // its target chosen for it — leaves the sheet. A send already in
            // flight is the one moment neither is allowed; [AmountStep] holds
            // its own handler for that, and being the inner one it wins.
            BackHandler {
                if (target != null && prefillAddress == null && prefillContact == null) {
                    target = null
                } else {
                    onDismiss()
                }
            }
            when (val t = target) {
                null -> ChooseTarget(
                    onPick = { t, amt -> target = t; scannedPxmr = amt },
                    onScan = { scanning = true },
                    onClose = onDismiss,
                )
                else -> AmountStep(
                    target = t,
                    prefillAmountPxmr = if (scannedPxmr > 0) scannedPxmr else prefillAmountPxmr,
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
/**
 * The address and the amount out of a `monero:` URI.
 *
 * The amount used to be thrown away. `substringBefore("?")` took the address
 * and dropped the query with it, so a code that had *said* what it wanted —
 * DUCAT writes `tx_amount` on every kiosk order, and so does every other
 * payment-request QR — landed on a pay screen with an empty amount field, and
 * the payer had to read the number off the merchant's screen and type it back
 * in. For a kiosk that is worse than clumsy: an order is attributed by its
 * exact amount, down to a sub-cent tag that makes it unique, so a hand-typed
 * round number is a payment the till can never match to the order it paid for.
 *
 * Returns null for anything that is not an address. The amount is 0 when the
 * URI did not name one, which is the same as "the payer decides".
 */
internal fun moneroUri(raw: String): Pair<String, Long>? {
    val t = raw.trim().removePrefix("monero:")
    val addr = t.substringBefore("?")
    if (addr.length !in 90..110) return null
    val q = t.substringAfter("?", "")
    // Only tx_amount. `amount` is not a Monero URI field, and guessing at one
    // would be inventing a request nobody made.
    val amount = q.split('&')
        .firstOrNull { it.startsWith("tx_amount=") }
        ?.removePrefix("tx_amount=")
        ?.let { java.net.URLDecoder.decode(it, "UTF-8") }
        ?.let { v -> runCatching { Amounts.toPxmr(java.math.BigDecimal(v)) }.getOrNull() }
        ?.takeIf { it > 0 }
        ?: 0L
    return addr to amount
}

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
    return moneroUri(t)?.let { (addr, _) -> PayTarget.ToAddress(addr) }
}

@Composable
private fun ChooseTarget(
    onPick: (PayTarget, Long) -> Unit,
    onScan: () -> Unit,
    onClose: () -> Unit,
) {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    // Alphabetical: a picker is looked *up*, not scrolled through in arrival
    // order — recency belongs to the chat list, names belong here.
    val contacts = remember(version) {
        ContactStore(context).all().sortedBy { it.displayName().lowercase() }
    }
    val ambiguous = remember(version) { ContactStore(context).ambiguous() }
    var address by rememberSaveable { mutableStateOf("") }

    Column(
        Modifier
            .fillMaxSize()
            .imePadding()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp)
            .padding(bottom = 24.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(top = 12.dp)) {
            IconButton(onClick = onClose) { Icon(Icons.Filled.Close, stringResource(R.string.pay_close)) }
            Spacer(Modifier.width(4.dp))
            Text(stringResource(R.string.pay_send_or_request), style = MaterialTheme.typography.titleLarge)
        }
        Spacer(Modifier.height(12.dp))

        OutlinedButton(onClick = onScan, modifier = Modifier.fillMaxWidth()) {
            Icon(Icons.Filled.QrCodeScanner, null, Modifier.size(18.dp))
            Spacer(Modifier.width(8.dp))
            Text(stringResource(R.string.pay_scan_a_code))
        }

        if (contacts.isNotEmpty()) {
            Spacer(Modifier.height(16.dp))
            Text(stringResource(R.string.pay_your_contacts), style = MaterialTheme.typography.labelLarge)
            Text(
                stringResource(R.string.pay_contact_vs_address),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(4.dp))
            // A plain Column, not a LazyColumn.
            //
            // The page already scrolls, and a lazy list inside a scroller of
            // the same direction has no height to be lazy against — hence the
            // 240dp cap that used to be here, which bought a second scroll
            // gesture fighting the page and a fourth contact sliced through the
            // middle, with half a screen of empty space below the button. A
            // contact list is a few dozen rows at the outside; the page scroller
            // can have all of them.
            contacts.forEach { c ->
                val shared = c.personaHex in ambiguous
                ListItem(
                    headlineContent = { Text(c.displayName()) },
                    supportingContent = {
                        // The key was always here, but a key nobody has a
                        // reason to read is furniture. When two rows carry the
                        // same name it is the only thing between them, so the
                        // row says which rows those are.
                        Text(
                            if (shared) {
                                stringResource(
                                    R.string.pay_name_shared_key,
                                    c.personaHex.take(16),
                                )
                            } else {
                                c.personaHex.take(16) + "…"
                            },
                            fontFamily = if (shared) FontFamily.Default else FontFamily.Monospace,
                            style = MaterialTheme.typography.bodySmall,
                            color =
                                if (shared) MaterialTheme.colorScheme.error
                                else MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                        )
                    },
                    leadingContent = { Avatar(c.displayName()) },
                    modifier = Modifier.clickable { onPick(PayTarget.ToContact(c), 0L) },
                )
            }
        }

        Spacer(Modifier.height(16.dp))
        Text(stringResource(R.string.pay_or_monero_address), style = MaterialTheme.typography.labelLarge)
        Spacer(Modifier.height(6.dp))
        OutlinedTextField(
            value = address,
            onValueChange = { address = it },
            placeholder = { Text(stringResource(R.string.pay_address_placeholder)) },
            modifier = Modifier.fillMaxWidth(),
            minLines = 2,
        )
        Spacer(Modifier.height(8.dp))
        // A pasted `monero:` link is an address too — and one that may name an
        // amount. Refusing it as "not a Monero address" was a dead end for the
        // ordinary act of copying a payment request.
        val pasted = moneroUri(address)
        Button(
            onClick = { pasted?.let { (a, amt) -> onPick(PayTarget.ToAddress(a), amt) } },
            enabled = pasted != null,
            modifier = Modifier.fillMaxWidth(),
        ) { Text(stringResource(R.string.pay_continue)) }
        if (address.isNotBlank() && pasted == null) {
            Text(
                stringResource(R.string.pay_not_monero_address),
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
    var fiatEntry by rememberSaveable { mutableStateOf(Amounts.enterFiat(context)) }
    val rate = remember(version) { RateStore(context).cached()?.first }
    val cur = remember { Amounts.currency(context) }
    var priority by remember { mutableIntStateOf(1) }

    // A prefilled amount is a *bill* — somebody else's number, answered rather
    // than edited. What stays yours is the tip. Editing the bill down and
    // paying anyway would make a payment nothing on their side can match; the
    // honest way to pay a different amount is a different payment.
    val billed = prefillAmountPxmr > 0
    var tipTyped by rememberSaveable { mutableStateOf("") }
    var typed by rememberSaveable {
        mutableStateOf(if (prefillAmountPxmr > 0) formatXmr(prefillAmountPxmr) else "")
    }
    var note by rememberSaveable { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var done by remember { mutableStateOf<String?>(null) }
    // The ceremony: a completed *send* takes the screen (Ceremony.kt). A
    // request keeps the quiet text — asking is not a crescendo.
    var paidPxmr by remember { mutableStateOf<Long?>(null) }
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
    // Between agreeing and spending: see the gate below.
    var askPin by remember { mutableStateOf(false) }
    val amountFocus = remember { FocusRequester() }

    LaunchedEffect(Unit) {
        if (billed) return@LaunchedEffect
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

    // And nothing leaves it while a payment is halfway out.
    //
    // Back steps the sheet back to the chooser or closes it outright, either
    // of which unmounts this screen — and unmounting cancels the scope
    // `doSend` is running in. Not the transaction: by then it is with a node
    // and cannot be taken back. What dies with the scope is everything after
    // it — the §16.13 kind-2 notice that names the txid, which is the only
    // way their wallet can put a sender on the output that just arrived, and
    // the arm that would have shown the error if there had been one. Money
    // gone, nothing in the thread, nothing on screen to say so.
    //
    // The few seconds this covers have a spinner on them and nothing else, so
    // there is no step to take and swallowing the press costs nobody a thing.
    BackHandler(enabled = busy) {}

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
            // This text is re-parsed as the amount, so it is pinned to the
            // dot the parser expects rather than the locale's separator, and
            // floored to the cent — a rounded-up cent would convert back to
            // more than the maximum just promised.
            typed = if (fiatEntry && rate != null) {
                "%.2f".format(
                    java.util.Locale.US,
                    kotlin.math.floor(maxPxmr / 1e12 * rate * 100) / 100,
                )
            } else formatXmr(maxPxmr)
        }
    }

    val tipPxmr = remember(tipTyped, fiatEntry, rate) {
        val v = moneyText(tipTyped).toBigDecimalOrNull() ?: return@remember 0L
        val xmr = if (fiatEntry && rate != null && rate > 0) {
            v.divide(BigDecimal.valueOf(rate), 12, java.math.RoundingMode.DOWN)
        } else if (fiatEntry) return@remember 0L else v
        Amounts.toPxmr(xmr)?.coerceAtLeast(0) ?: 0L
    }
    val pxmr = remember(typed, fiatEntry, rate, tipPxmr, billed) {
        if (billed) return@remember prefillAmountPxmr + tipPxmr
        val v = moneyText(typed).toBigDecimalOrNull() ?: return@remember null
        val xmr = if (fiatEntry && rate != null && rate > 0) {
            v.divide(BigDecimal.valueOf(rate), 12, java.math.RoundingMode.DOWN)
        } else v
        xmr.multiply(BigDecimal(1_000_000_000_000L)).toLong().takeIf { it > 0 }
    }

    var quote by remember { mutableStateOf<Quote?>(null) }
    LaunchedEffect(pxmr, priority, version) {
        val amt = pxmr
        quote = if (amt == null) null
        else withContext(Dispatchers.IO) { Wallet.quote(context, amt, priority) }
    }

    // Sending is capped by what this wallet holds; asking is not — the payer's
    // balance is the payer's problem, and a request for more than *you* hold is
    // the ordinary case (that is usually why you are asking). The red state
    // keyed on the cap regardless of mode, so typing 50 into a request lit the
    // field up as an error against a wallet that is not even the one paying.
    val overMax = !asking && pxmr != null && maxPxmr > 0 && pxmr > maxPxmr

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
            IconButton(onClick = onBack) { Icon(Icons.Filled.ArrowBack, stringResource(R.string.pay_back)) }
            Spacer(Modifier.width(4.dp))
            when (target) {
                is PayTarget.ToContact -> {
                    Avatar(target.contact.displayName())
                    Spacer(Modifier.width(10.dp))
                    // Somebody else in the list reads the same on screen, so
                    // the name on this header is not enough to know who is
                    // about to be paid. Say so here rather than only in the
                    // picker: this is the screen with the amount on it, and
                    // it is the last one before the money goes.
                    val shared = remember(target.contact.personaHex, version) {
                        target.contact.personaHex in ContactStore(context).ambiguous()
                    }
                    // A card tried to move where this contact gets paid and
                    // was held. Said here as well as on their profile, because
                    // this is the screen with the amount on it — and it wins
                    // the slot over a shared name, which costs a mis-tap where
                    // this costs the payment.
                    val held = target.contact.pendingAddress != null
                    Column {
                        Text(target.contact.displayName(),
                             style = MaterialTheme.typography.titleMedium)
                        when {
                            held -> Text(
                                stringResource(R.string.pay_payto_held),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.error,
                            )
                            shared -> {
                                Text(
                                    stringResource(R.string.pay_name_shared),
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.error,
                                )
                                Text(
                                    target.contact.personaHex.take(24).chunked(4).joinToString(" "),
                                    fontFamily = FontFamily.Monospace,
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.outline,
                                )
                            }
                            else -> Text(
                                stringResource(R.string.pay_in_ducat),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.outline,
                            )
                        }
                    }
                }
                is PayTarget.ToAddress -> Column {
                    Text(stringResource(R.string.pay_monero_address), style = MaterialTheme.typography.titleSmall)
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
        if (billed) {
            // Their number, shown as a fact rather than a field. What is
            // editable on a bill is the tip, and only the tip.
            Text(
                stringResource(R.string.pay_bill),
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            val billShown = Amounts.show(context, prefillAmountPxmr)
            Text(billShown.primary, style = MaterialTheme.typography.displayMedium)
            billShown.secondary?.let {
                Text(it, style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Spacer(Modifier.height(14.dp))
            OutlinedTextField(
                value = tipTyped,
                onValueChange = { tipTyped = it.filter { c -> Amounts.isNumberChar(c) } },
                label = { Text(stringResource(R.string.pay_add_tip, if (fiatEntry) cur else "XMR")) },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                modifier = Modifier.fillMaxWidth(),
            )
            if (tipPxmr > 0) {
                Spacer(Modifier.height(6.dp))
                val t = Amounts.show(context, pxmr ?: 0L)
                Text(
                    stringResource(R.string.pay_total_with_tip, t.primary) +
                        (t.secondary?.let { " · $it" } ?: ""),
                    style = MaterialTheme.typography.titleMedium,
                )
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (!billed) {
            OutlinedTextField(
                value = typed,
                onValueChange = {
                    typed = it.filter { c -> Amounts.isNumberChar(c) }
                    maxLocked = false
                },
                placeholder = { Text(stringResource(R.string.pay_amount_placeholder)) },
                textStyle = MaterialTheme.typography.displayMedium,
                singleLine = true,
                isError = overMax,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                modifier = Modifier.weight(1f).focusRequester(amountFocus),
            )
            }
            Spacer(Modifier.width(10.dp))
            if (!billed && rate != null) {
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
            } else if (!billed) {
                // The unit beside the amount field, for when there is no rate
                // to toggle against. `billed` hides the field, and this used to
                // print "XMR" beside the space where it wasn't — a label with
                // nothing to label, directly above a conversion line already
                // shown under the bill.
                Text("XMR", style = MaterialTheme.typography.labelLarge)
            }
        }

        // The other unit, live, so nobody converts in their head to check.
        //
        // Only where there is an amount being typed. A bill already prints both
        // units under its own total, and adds a second line with both when a
        // tip is entered — so this one said the same number a third time, and
        // on a bill with no tip it sat directly under the first one.
        if (!billed) pxmr?.let {
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
                stringResource(
                    R.string.pay_asking_explainer,
                    (target as? PayTarget.ToContact)?.contact?.displayName()
                        ?: stringResource(R.string.pay_them),
                ),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        } else {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                stringResource(R.string.pay_up_to_after_fees, inUnit(context, maxPxmr, fiatEntry, rate, cur)),
                style = MaterialTheme.typography.bodySmall,
                color = if (overMax) MaterialTheme.colorScheme.error
                        else MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.weight(1f))
            // Not on a bill. Max writes the wallet's maximum into `typed`, and
            // a billed screen computes its amount from the bill plus the tip
            // and never reads `typed` — so the button did nothing at all, on
            // the one screen where a control that looks like it changes the
            // amount had better not.
            //
            // The headroom line beside it stays: on a bill it is the room for
            // the bill *and* the tip, it turns red when they exceed it, and
            // that is what disables Send.
            if (!billed) {
                TextButton(
                    onClick = { maxLocked = true },
                    enabled = maxPxmr > 0,
                ) { Text(if (maxLocked) stringResource(R.string.pay_max_done) else stringResource(R.string.pay_max)) }
            }
        }
        if (overMax) {
            Text(
                stringResource(R.string.pay_over_max),
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
                    CostRow(stringResource(R.string.pay_amount), Amounts.show(context, q.amountPxmr).primary)
                    // A fee of zero is the estimator saying it could not reach a
                    // node, not Monero being free. Printing it as a number made
                    // the total below it a promise this screen cannot keep — the
                    // send that follows costs whatever the transaction costs.
                    CostRow(
                        stringResource(R.string.pay_network_fee_estimated),
                        if (q.feeKnown) Amounts.show(context, q.feePxmr).primary
                        else stringResource(R.string.pay_fee_unknown),
                    )
                    HorizontalDivider(Modifier.padding(vertical = 8.dp))
                    if (q.feeKnown) {
                        CostRow(
                            stringResource(R.string.pay_total),
                            Amounts.show(context, q.totalPxmr).primary,
                            bold = true,
                        )
                    }
                    // Amount, fee and total explain *why* something is out of
                    // reach — the fee is usually what tips it over. The rest
                    // describes a transaction that cannot happen, so it is left
                    // out rather than invented: "left after" is clamped at zero
                    // and would claim this send lands you at exactly nothing,
                    // and the note count and timing price a plan nobody can
                    // buy. The line above the card says what is wrong.
                    if (q.affordable && q.feeKnown) {
                        CostRow(
                            stringResource(R.string.pay_left_after),
                            Amounts.show(context, q.remainingPxmr).primary,
                        )
                        Spacer(Modifier.height(8.dp))
                        Text(
                            stringResource(R.string.pay_uses_notes, q.notes, q.minutesToConfirm),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Text(
                            stringResource(R.string.pay_fee_estimate_note),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.outline,
                        )
                        // The rate this screen converted at, and who said so.
                        //
                        // Everything above is a conversion, and until now the
                        // number it was converted at appeared nowhere. A wrong
                        // rate — one venue having a bad day, or lying — reads
                        // as a perfectly ordinary payment, because the fiat
                        // figure is exactly the one the payer typed. Naming it
                        // here is the last place it can be caught by somebody
                        // who knows roughly what Monero costs.
                        if (rate != null) {
                            val src = org.ducatproject.ducat.RateStore(context).source()
                            Text(
                                if (src.isBlank()) {
                                    stringResource(R.string.pay_rate_line, cur, fmtRate(rate))
                                } else {
                                    stringResource(
                                        R.string.pay_rate_line_source, cur, fmtRate(rate), src,
                                    )
                                },
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.outline,
                            )
                        }
                    }
                }
            }

            Spacer(Modifier.height(10.dp))
            Text(stringResource(R.string.pay_speed), style = MaterialTheme.typography.labelLarge)
            Spacer(Modifier.height(4.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                listOf(
                    stringResource(R.string.pay_speed_slow),
                    stringResource(R.string.pay_speed_normal),
                    stringResource(R.string.pay_speed_fast),
                    stringResource(R.string.pay_speed_fastest),
                ).forEachIndexed { i, label ->
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
                // The memo. It rides the sealed notice, never the chain — a
                // public memo field would be a note stapled to a banknote —
                // and Activity shows it on the transaction like a bank line.
                label = { Text(stringResource(R.string.pay_memo_label)) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
        }

        Spacer(Modifier.height(16.dp))
        // The mode, where PayPal puts it: a pill above the action, so the two
        // verbs are one visible choice rather than two competing buttons.
        if (canAsk && !billed) {
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                SegmentedButton(
                    selected = !asking,
                    onClick = { asking = false },
                    shape = SegmentedButtonDefaults.itemShape(0, 2),
                    // No checkmark — the fill already says which is active,
                    // and the icon shoves the label sideways when it appears.
                    icon = {},
                    modifier = Modifier.weight(1f),
                ) { Text(stringResource(R.string.pay_send), maxLines = 1, softWrap = false) }
                SegmentedButton(
                    selected = asking,
                    onClick = { asking = true },
                    shape = SegmentedButtonDefaults.itemShape(1, 2),
                    // No checkmark — the fill already says which is active,
                    // and the icon shoves the label sideways when it appears.
                    icon = {},
                    modifier = Modifier.weight(1f),
                ) { Text(stringResource(R.string.pay_request), maxLines = 1, softWrap = false) }
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
                                        note.ifBlank { context.getString(R.string.pay_payment_request) },
                                        PersonaStore(context).personaHex(),
                                        kind = 1, amountPxmr = amt,
                                        payto = WalletStore(context)
                                            .addressFor(target.contact.personaHex),
                                    )
                                }
                            }
                            busy = false
                            r.onSuccess { done = context.getString(R.string.pay_request_sent) }
                                .onFailure { error = sendFailure(context, it) }
                        }
                    },
                    // Asking for more than you hold is perfectly reasonable, so
                    // a request is never blocked by the balance. It is blocked
                    // by its own success: the form lingers while "Request sent"
                    // is read, and a second tap there is a duplicate bill.
                    enabled = !busy && done == null && pxmr != null,
                    modifier = Modifier.weight(1f).height(52.dp),
                ) {
                    if (busy) CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                    else Text(stringResource(R.string.pay_request))
                }
            } else {
                val payable = target !is PayTarget.ToContact ||
                    target.contact.theirAddress != null || prefillAmountPxmr > 0
                Button(
                    onClick = { confirming = true },
                    enabled = !busy && done == null && pxmr != null && payable && !overMax &&
                        quote?.affordable == true,
                    modifier = Modifier.weight(1f).height(52.dp),
                ) {
                    if (busy) CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                    else Text(stringResource(R.string.pay_send))
                }
            }
        }

        if (!asking && target is PayTarget.ToContact && target.contact.theirAddress == null &&
            prefillAmountPxmr == 0L
        ) {
            Spacer(Modifier.height(8.dp))
            Text(
                stringResource(R.string.pay_no_address_hint, target.contact.displayName()),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        done?.let {
            Spacer(Modifier.height(12.dp))
            Text(it, color = MaterialTheme.ducat.settled)
        }
        // The minute the money is in flight, on the whole screen rather than
        // inside a button. The paid splash is this same object resolving, so
        // one becomes the other without the eye losing what it was following.
        if (busy && pxmr != null && paidPxmr == null) {
            SendingSplash(
                amountPxmr = pxmr,
                toName = (target as? PayTarget.ToContact)?.contact?.displayName(),
            )
        }
        paidPxmr?.let { amt ->
            PaidSplash(
                amountPxmr = amt,
                toName = (target as? PayTarget.ToContact)?.contact?.displayName(),
                onDone = { paidPxmr = null; onDone() },
            )
        }
        error?.let {
            Spacer(Modifier.height(12.dp))
            Text(it, color = MaterialTheme.colorScheme.error,
                 style = MaterialTheme.typography.bodySmall)
        }
    }

        // Where it is going, worked out once so both the confirmation and
        // the send agree about it.
        val dest = when (target) {
            is PayTarget.ToContact -> target.contact.theirAddress
            is PayTarget.ToAddress -> target.address
        }
        // The PIN sits between agreeing to a payment and making it, not in
        // front of the app: what needs proving is that the person spending
        // is the owner, and this is the moment that is true.
        val doSend: () -> Unit = doSend@{
            val amount = pxmr ?: return@doSend
                busy = true; error = null
                scope.launch {
                    val r = withContext(Dispatchers.IO) {
                        runCatching {
                            // A demoted node leaves lastGood empty; the user's
                            // retry deserves a fresh probe, not "no node".
                            val node = NodeStore(context).lastGood()
                                ?: runCatching {
                                    uniffi.ducat_mobile.moneroPickNode(
                                        uniffi.ducat_mobile.moneroDefaultNodes(
                                            NodeStore(context).ownUrl()),
                                        "stagenet", 8000u,
                                    ).also {
                                        NodeStore(context).rememberLastGood(it.url)
                                    }.url
                                }.getOrNull()
                                ?: throw IllegalStateException(
                                    context.getString(R.string.pay_no_node)
                                )
                            val to = dest
                                ?: throw IllegalStateException(
                                    context.getString(R.string.pay_no_address_error)
                                )
                            val contact = (target as? PayTarget.ToContact)?.contact
                            val res = Wallet.send(
                                context, node, to, amount,
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
                                        note.ifBlank { context.getString(R.string.pay_payment) },
                                        PersonaStore(context).personaHex(),
                                        kind = 2, amountPxmr = amount,
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
                    r.onSuccess { paidPxmr = amount }
                        .onFailure { error = sendFailure(context, it) }
                }
        }
    // Outside the confirm, because it outlives it.
    //
    // `onConfirm` closes the dialog and opens the gate, in that order and in
    // one frame. With the gate rendered *inside* `if (confirming)`, closing
    // the dialog unmounted the gate in the same recomposition — so `askPin`
    // went true against nothing, no PIN was ever asked for, and `doSend` was
    // never reached. Pressing Send on the confirmation did nothing at all:
    // no payment, no PIN, no error, back to the bill. Found paying a USD 3.20
    // coffee at the till, twice, before looking at why.
    PinGate(
        open = askPin,
        onDismiss = { askPin = false },
        onPassed = { askPin = false; doSend() },
    )
    if (confirming && pxmr != null) {
        ConfirmSend(
            pxmr = pxmr,
            quote = quote,
            destination = dest,
            contactName = (target as? PayTarget.ToContact)?.contact?.displayName(),
            busy = busy,
            onCancel = { confirming = false },
            onConfirm = latch@{
                // Latched before anything else, synchronously: a second tap in
                // the frame before this dialog leaves would build and broadcast
                // a second transaction, and nothing downstream can take one
                // back.
                if (busy) return@latch
                confirming = false
                askPin = true
            },
        )
    }
}

/**
 * Typed money, made parseable.
 *
 * Decimal keyboards follow the device locale, and many of them offer a comma
 * where the parser expects a dot — a filter that dropped the comma made cents
 * untypeable. The fields accept both marks; this is the one place a comma
 * becomes a dot, so every parse sees the same shape.
 */
// One reader for every amount anyone types. `replace(',', '.')` handled
// exactly one of the world's decimal separators; see Amounts.typedNumber.
internal fun moneyText(s: String): String = Amounts.typedNumber(s)

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
    busy: Boolean,
    onCancel: () -> Unit,
    onConfirm: () -> Unit,
) {
    val context = LocalContext.current
    val a = Amounts.show(context, pxmr)
    AlertDialog(
        onDismissRequest = onCancel,
        title = {
            Column {
                Text(stringResource(R.string.pay_confirm_send_title, a.primary))
                a.secondary?.let {
                    Text(it, style = MaterialTheme.typography.labelMedium,
                         color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        },
        text = {
            Column {
                contactName?.let { Text(stringResource(R.string.pay_to, it), style = MaterialTheme.typography.bodyMedium) }
                destination?.let {
                    Spacer(Modifier.height(6.dp))
                    Text(it, fontFamily = FontFamily.Monospace,
                         style = MaterialTheme.typography.bodySmall)
                }
                quote?.let { q ->
                    Spacer(Modifier.height(10.dp))
                    Text(
                        // The last screen before the money leaves is the worst
                        // place to state a total built on a fee nobody could
                        // fetch. Unknown says unknown here too.
                        if (!q.feeKnown) stringResource(R.string.pay_fee_unknown_confirm)
                        else stringResource(
                            R.string.pay_plus_fees,
                            Amounts.show(context, q.feePxmr).primary,
                            Amounts.show(context, q.totalPxmr).primary,
                        ),
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                Spacer(Modifier.height(10.dp))
                Text(
                    stringResource(R.string.pay_irreversible_warning),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.ducat.changePending,
                )
            }
        },
        // Disabled once a send is in flight — the second half of the
        // double-tap guard, alongside the latch in onConfirm itself.
        confirmButton = { TextButton(onClick = onConfirm, enabled = !busy) { Text(stringResource(R.string.pay_send)) } },
        dismissButton = { TextButton(onClick = onCancel) { Text(stringResource(R.string.pay_cancel)) } },
    )
}

/**
 * A send failure, in words instead of in the transport's own vocabulary.
 *
 * What a phone showed while trying to pay: `v1=decoys:
 * InterfaceError(InterfaceError("timed out reading response"))`. That is a
 * public node failing on `get_outs`, the heaviest call a send makes — nothing
 * to do with the wallet, the amount, or the address, and nothing a person can
 * act on as written. The app has already demoted that node by the time this
 * runs, so the useful thing to say is: try again, it will use another.
 */
private fun sendFailure(context: android.content.Context, t: Throwable): String = when {
    Wallet.isNodeTrouble(t) -> context.getString(R.string.pay_node_no_answer)
    else -> t.saidWhy() ?: context.getString(R.string.pay_could_not_send)
}

/** A rate as a person reads it: two decimals, no exponent, whatever the size. */
private fun fmtRate(r: Double): String =
    java.math.BigDecimal(r).setScale(2, java.math.RoundingMode.HALF_UP).toPlainString()
