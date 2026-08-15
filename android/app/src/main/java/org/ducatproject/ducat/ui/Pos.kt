package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Nfc
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.BillItem
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.RateStore
import org.ducatproject.ducat.TabStore
import org.ducatproject.ducat.formatXmr
import java.math.BigDecimal

private const val TAG = "POS"

/**
 * The till (§15's POS mode).
 *
 * The whole sale is one path with no dead ends: ring up lines, one button, one
 * code, and the bill, the payment and the receipt all travel the same
 * conversation the code opens. The person behind the counter does this forty
 * times a shift, so every step that can happen by itself does — the bill sends
 * on scan, the receipt sends on payment.
 */
@Composable
fun PosScreen() {
    val context = LocalContext.current
    var basket by remember { mutableStateOf(listOf<BillItem>()) }
    var taxPxmr by remember { mutableStateOf(0L) }
    var charging by remember { mutableStateOf(false) }
    // Two registers, one till: itemised for the shop that rings up lines,
    // quick for the coffee cart that only ever needs a number. Both end in
    // the same card, the same bill, the same receipt — quick just bills one
    // line named "Sale", because a §16.13 bill must still add up.
    var quick by remember { mutableStateOf(false) }
    var quickAmount by remember { mutableStateOf("") }
    var quickFiat by remember { mutableStateOf(Amounts.preferFiat(context)) }

    val total = basket.sumOf { it.amountPxmr } + taxPxmr

    if (charging) {
        PresentScreen(
            items = basket,
            taxPxmr = taxPxmr.takeIf { it > 0 },
            totalPxmr = total,
            onDone = { charging = false; basket = emptyList(); taxPxmr = 0L },
            onBack = { charging = false },
        )
        return
    }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        // The running total is the hero, like every other screen's number.
        Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp)) {
            Spacer(Modifier.height(16.dp))
            Text(
                "This sale",
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            val shown = Amounts.show(context, total)
            Text(shown.primary, style = MaterialTheme.typography.displayLarge)
            shown.secondary?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.height(12.dp))
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                SegmentedButton(
                    selected = !quick, onClick = { quick = false },
                    shape = SegmentedButtonDefaults.itemShape(0, 2),
                    icon = {},
                ) { Text("Items") }
                SegmentedButton(
                    selected = quick, onClick = { quick = true },
                    shape = SegmentedButtonDefaults.itemShape(1, 2),
                    icon = {},
                ) { Text("Quick amount") }
            }
            Spacer(Modifier.height(12.dp))
        }

        if (quick) {
            val rate = remember { RateStore(context).cached()?.first }
            val cur = remember { Amounts.currency(context) }
            Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    OutlinedTextField(
                        value = quickAmount,
                        onValueChange = {
                            quickAmount = it.filter { c -> c.isDigit() || c == '.' || c == ',' }
                        },
                        label = { Text(if (quickFiat) "Total ($cur)" else "Total (XMR)") },
                        keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                            keyboardType = androidx.compose.ui.text.input.KeyboardType.Decimal,
                        ),
                        modifier = Modifier.weight(1f),
                        singleLine = true,
                        textStyle = MaterialTheme.typography.headlineSmall,
                    )
                    if (rate != null) {
                        Spacer(Modifier.width(8.dp))
                        TextButton(onClick = { quickFiat = !quickFiat }) {
                            Text(if (quickFiat) cur else "XMR")
                        }
                    }
                }
                Spacer(Modifier.height(12.dp))
                val pxmr = moneyText(quickAmount).toDoubleOrNull()?.let { v ->
                    if (quickFiat && rate != null) ((v / rate) * 1e12).toLong()
                    else if (!quickFiat) (v * 1e12).toLong()
                    else null
                }?.takeIf { it > 0 }
                Button(
                    onClick = {
                        basket = listOf(BillItem("Sale", pxmr!!))
                        taxPxmr = 0L
                        quickAmount = ""
                        charging = true
                    },
                    enabled = pxmr != null,
                    modifier = Modifier.fillMaxWidth().height(56.dp),
                ) {
                    Text(
                        pxmr?.let { "Request ${Amounts.show(context, it).primary}" }
                            ?: "Request payment",
                        style = MaterialTheme.typography.labelLarge,
                    )
                }
                Spacer(Modifier.height(24.dp))
            }
            return@Column
        }

        PosAddLine { d, a -> basket = basket + BillItem(d, a) }

        if (basket.isNotEmpty()) {
            Spacer(Modifier.height(12.dp))
            Card(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
                Column(Modifier.padding(vertical = 6.dp, horizontal = 16.dp)) {
                    basket.forEachIndexed { i, item ->
                        Row(
                            Modifier.fillMaxWidth().padding(vertical = 6.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                item.description,
                                style = MaterialTheme.typography.bodyLarge,
                                modifier = Modifier.weight(1f),
                            )
                            AmountBoth(item.amountPxmr)
                            IconButton(
                                onClick = { basket = basket.filterIndexed { j, _ -> j != i } },
                                modifier = Modifier.size(32.dp),
                            ) { Icon(Icons.Filled.Close, "Remove", Modifier.size(16.dp)) }
                        }
                    }
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                    TaxRow(taxPxmr) { taxPxmr = it }
                }
            }

            Spacer(Modifier.height(8.dp))
            // A Monero fee is the sender's, paid to the network. On a bill it
            // would be charged twice: once in the total and again when the
            // customer's wallet builds the transaction. §16.13 has no field
            // for it for exactly this reason.
            Text(
                "The network fee is the customer's, paid to the network — never " +
                    "part of the bill.",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.outline,
                modifier = Modifier.padding(horizontal = 24.dp),
            )

            Spacer(Modifier.height(16.dp))
            Button(
                onClick = { charging = true },
                enabled = total > 0,
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp).height(56.dp),
            ) {
                Text(
                    "Charge ${Amounts.show(context, total).primary}",
                    style = MaterialTheme.typography.labelLarge,
                )
            }
            Spacer(Modifier.height(24.dp))
        } else {
            Spacer(Modifier.height(12.dp))
            Text(
                "Add the first item to start a sale.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
        }
    }
}

/** Both units on one line, XMR mono with the fiat quietly under it. */
@Composable
private fun AmountBoth(pxmr: Long) {
    val context = LocalContext.current
    Column(horizontalAlignment = Alignment.End) {
        Text(
            "${formatXmr(pxmr)} XMR",
            style = MaterialTheme.typography.bodyMedium,
            fontFamily = FontFamily.Monospace,
        )
        Amounts.show(context, pxmr).secondary?.let {
            Text(
                it,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.outline,
            )
        }
    }
}

/**
 * One line of the bill, priced in whichever unit the till thinks in.
 *
 * The unit toggle matches the pay screen's: a shop prices in the local
 * currency, a crypto meet prices in XMR, and both are one tap from each other.
 * Whatever is typed, the line is *stored* in piconero — §18.2's integers — and
 * both units show on every row after that.
 */
@Composable
internal fun PosAddLine(onAdd: (String, Long) -> Unit) {
    val context = LocalContext.current
    var desc by remember { mutableStateOf("") }
    var amount by remember { mutableStateOf("") }
    var fiat by remember { mutableStateOf(Amounts.preferFiat(context)) }
    val rate = remember { RateStore(context).cached()?.first }
    val cur = remember { Amounts.currency(context) }

    val pxmr: Long? = remember(amount, fiat, rate) {
        val v = moneyText(amount).toBigDecimalOrNull() ?: return@remember null
        val xmr = if (fiat) {
            if (rate == null || rate <= 0) return@remember null
            v.divide(BigDecimal(rate), 12, java.math.RoundingMode.DOWN)
        } else v
        runCatching { xmr.movePointRight(12).toLong() }.getOrNull()?.takeIf { it > 0 }
    }

    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        OutlinedTextField(
            value = desc,
            onValueChange = { if (it.length <= 64) desc = it },
            label = { Text("Item") },
            singleLine = true,
            modifier = Modifier.weight(1.5f),
        )
        Spacer(Modifier.width(8.dp))
        OutlinedTextField(
            value = amount,
            onValueChange = { amount = it.filter { c -> c.isDigit() || c == '.' || c == ',' } },
            label = { Text(if (fiat) cur else "XMR") },
            singleLine = true,
            modifier = Modifier.weight(1f),
        )
        if (rate != null) {
            TextButton(
                onClick = { fiat = !fiat; amount = "" },
                contentPadding = PaddingValues(horizontal = 6.dp),
            ) {
                Text(if (fiat) "→XMR" else "→$cur", style = MaterialTheme.typography.labelMedium)
            }
        }
        FilledIconButton(
            onClick = { onAdd(desc.trim(), pxmr!!); desc = ""; amount = "" },
            enabled = desc.isNotBlank() && pxmr != null,
        ) { Icon(Icons.Filled.Add, "Add line") }
    }
}

@Composable
private fun TaxRow(taxPxmr: Long, onSet: (Long) -> Unit) {
    val context = LocalContext.current
    var text by remember { mutableStateOf(if (taxPxmr > 0) formatXmr(taxPxmr) else "") }
    Row(
        Modifier.fillMaxWidth().padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text("Tax", style = MaterialTheme.typography.bodyLarge)
            if (taxPxmr > 0) {
                Amounts.show(context, taxPxmr).secondary?.let {
                    Text(
                        it,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.outline,
                    )
                }
            }
        }
        OutlinedTextField(
            value = text,
            onValueChange = {
                text = it.filter { c -> c.isDigit() || c == '.' || c == ',' }
                onSet(moneyText(text).toDoubleOrNull()?.let { v -> (v * 1e12).toLong() } ?: 0L)
            },
            label = { Text("XMR") },
            placeholder = { Text("0") },
            singleLine = true,
            modifier = Modifier.width(150.dp),
        )
    }
}

/** Where the sale stands, so the screen can say it in one word. */
private enum class Sale { Waiting, Billed, Seen, Paid }

/**
 * The bill on screen, one code under it, and the rest happens by itself.
 *
 * Scan → they become a contact and the itemised bill lands in the new
 * conversation. Pay → the till sees the amount arrive on chain and sends the
 * receipt (§16.13's `RECEIPT`, the claim only the payee can make), pointing at
 * the transaction it acknowledges. The screen narrates each step because the
 * vendor cannot see the customer's phone.
 */
@Composable
private fun PresentScreen(
    items: List<BillItem>,
    taxPxmr: Long?,
    totalPxmr: Long,
    onDone: () -> Unit,
    onBack: () -> Unit,
) {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    var cardUri by remember { mutableStateOf<String?>(null) }
    var cardInbox by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var customer by remember { mutableStateOf<Contact?>(null) }
    var saleTabId by remember { mutableStateOf<String?>(null) }

    // The stage is *derived* from the settlement store, which the poller
    // drives — so a sale marked paid while this screen was backgrounded shows
    // paid the moment it returns, and the receipt logic lives in exactly one
    // place (TabStore.reconcile) for every vendor mode.
    val stage = when {
        saleTabId != null -> remember(version, saleTabId) {
            val t = TabStore(context).get(saleTabId!!)
            when {
                t?.state == "paid" -> Sale.Paid
                t?.seenTx != null -> Sale.Seen
                else -> Sale.Billed
            }
        }
        else -> Sale.Waiting
    }

    // While the code is up, a tap offers the same sale card.
    DisposableEffect(cardUri) {
        org.ducatproject.ducat.nfc.Tap.offered = cardUri
        onDispose { org.ducatproject.ducat.nfc.Tap.offered = null }
    }

    // A card per sale, marked as one: a "sale" card never auto-reissues, and
    // this flow waits for *its* claimant — a profile-code scan mid-sale must
    // not be billed as the customer.
    LaunchedEffect(Unit) {
        val r = withContext(Dispatchers.IO) {
            runCatching {
                Mailbox.issueCard(
                    context, MyProfile(context).name(), 60uL * 60uL * 2uL, purpose = "sale",
                )
            }
        }
        r.onSuccess { cardUri = it.uri; cardInbox = it.inboxKey }
            .onFailure {
                error = it.message ?: "could not publish the code"
                DucatLog.e(TAG, "card: ${it.message}")
            }
    }

    // Scan → bill. The claim bound to this card makes them a contact; the
    // bill lands in the new conversation, itemised, destination inside it,
    // and the sale becomes a settlement the poller watches.
    LaunchedEffect(cardInbox) {
        val inbox = cardInbox ?: return@LaunchedEffect
        while (customer == null) {
            delay(2_000)
            val fresh = withContext(Dispatchers.IO) {
                runCatching {
                    Mailbox.collectClaims(context)
                    ContactStore(context).claimantOf(inbox)?.let { hex ->
                        ContactStore(context).all().firstOrNull { it.personaHex == hex }
                    }
                }.getOrNull()
            } ?: continue
            withContext(Dispatchers.IO) {
                runCatching {
                    val store = TabStore(context)
                    val tab = store.open(fresh.personaHex, "pos")
                    store.update(store.get(tab.id)!!.copy(lines = items, taxPxmr = taxPxmr))
                    store.settle(store.get(tab.id)!!)
                    tab.id
                }.onSuccess { id ->
                    customer = fresh
                    saleTabId = id
                    DucatLog.i(TAG, "billed ${fresh.displayName()} ${formatXmr(totalPxmr)} XMR")
                }.onFailure {
                    error = "They connected, but the bill did not send: ${it.message}"
                    DucatLog.e(TAG, "bill: ${it.message}")
                }
            }
        }
    }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Spacer(Modifier.height(8.dp))
        val shown = Amounts.show(context, totalPxmr)
        Text(shown.primary, style = MaterialTheme.typography.displayMedium)
        shown.secondary?.let {
            Text(it, style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant)
        }

        // The bill, exactly as the customer will receive it.
        Spacer(Modifier.height(14.dp))
        Card(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(16.dp)) {
                items.forEach { i ->
                    Row(Modifier.fillMaxWidth().padding(vertical = 2.dp)) {
                        Text(i.description, Modifier.weight(1f),
                            style = MaterialTheme.typography.bodyMedium)
                        Text("${formatXmr(i.amountPxmr)} XMR",
                            style = MaterialTheme.typography.bodyMedium,
                            fontFamily = FontFamily.Monospace)
                    }
                }
                taxPxmr?.let {
                    Row(Modifier.fillMaxWidth().padding(vertical = 2.dp)) {
                        Text("Tax", Modifier.weight(1f),
                            style = MaterialTheme.typography.bodyMedium)
                        Text("${formatXmr(it)} XMR",
                            style = MaterialTheme.typography.bodyMedium,
                            fontFamily = FontFamily.Monospace)
                    }
                }
            }
        }

        Spacer(Modifier.height(16.dp))
        when (stage) {
            Sale.Waiting -> when {
                cardUri == null && error == null -> {
                    CircularProgressIndicator()
                    Spacer(Modifier.height(10.dp))
                    Text("Getting the code ready…",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                cardUri != null -> {
                    QrBlock(cardUri!!)
                    Spacer(Modifier.height(10.dp))
                    Text(
                        "They scan this in DUCAT. The bill above arrives on their " +
                            "phone the moment they do.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                    )
                    Spacer(Modifier.height(6.dp))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Filled.Nfc, null, Modifier.size(14.dp),
                            tint = MaterialTheme.colorScheme.outline)
                        Spacer(Modifier.width(6.dp))
                        Text(
                            "Or tap phones — same bill either way",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.outline,
                        )
                    }
                }
                else -> {}
            }
            Sale.Seen -> {
                Text(
                    "Payment seen ✓",
                    style = MaterialTheme.typography.headlineSmall,
                    color = MaterialTheme.ducat.changePending,
                )
                Spacer(Modifier.height(4.dp))
                Text(
                    "Their payment is in the network — settling, about two " +
                        "minutes. The receipt sends itself when it lands.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
                Spacer(Modifier.height(10.dp))
                CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
            }
            Sale.Billed -> {
                Text("Bill sent to ${customer?.displayName() ?: "the customer"}",
                    style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(6.dp))
                Text(
                    "They confirm on their own phone. When the payment lands on " +
                        "chain the receipt goes to them automatically — nothing " +
                        "left to do here.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
                Spacer(Modifier.height(10.dp))
                CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
            }
            Sale.Paid -> {
                Text("Paid ✓", style = MaterialTheme.typography.headlineMedium,
                    color = MaterialTheme.ducat.settled)
                Spacer(Modifier.height(4.dp))
                Text(
                    "Receipt sent to ${customer?.displayName() ?: "the customer"}.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        error?.let {
            Spacer(Modifier.height(10.dp))
            Text(it, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall)
        }

        Spacer(Modifier.height(20.dp))
        val scope = rememberCoroutineScope()
        // Leaving a billed sale must withdraw the bill, not just the screen.
        // The settled record would otherwise sit in the store forever, and a
        // later unrelated payment of the same amount would match it — a
        // receipt fired into a dead sale's thread.
        fun abandon() {
            saleTabId?.let { id ->
                scope.launch(Dispatchers.IO) {
                    runCatching {
                        val store = TabStore(context)
                        store.get(id)
                            ?.takeIf { it.state == "settled" }
                            // The retract path: the customer's Review button
                            // greys out by itself when the bill can be named.
                            ?.let { cancelTabWithRetract(context, store, it) }
                    }
                }
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            if (stage == Sale.Waiting || stage == Sale.Billed) {
                OutlinedButton(
                    onClick = { abandon(); onBack() },
                    modifier = Modifier.weight(1f).height(48.dp),
                ) { Text("Back") }
            }
            Button(
                onClick = {
                    // Once a payment is sighted, the money is in flight and
                    // cancelling the bill would orphan it — the way out is
                    // letting it settle.
                    if (stage == Sale.Waiting || stage == Sale.Billed) abandon()
                    onDone()
                },
                modifier = Modifier.weight(1f).height(48.dp),
            ) {
                Text(
                    when (stage) {
                        Sale.Paid -> "New sale"
                        Sale.Seen -> "New sale (this one settles by itself)"
                        else -> "Cancel sale"
                    },
                    maxLines = 1, softWrap = false,
                    style = MaterialTheme.typography.labelMedium,
                )
            }
        }
        Spacer(Modifier.height(24.dp))
    }
}
