package org.ducatproject.ducat.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Nfc
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
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
import org.ducatproject.ducat.R
import org.ducatproject.ducat.RateStore
import org.ducatproject.ducat.RunningTab
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
/** The basket across recreation and process death: a till is a tablet that
 *  rotates and a phone Android reclaims mid-rush, and a rung-up sale is real
 *  work. Flattened to [desc, amount, desc, amount, …] — both bundle-safe. */
internal val BasketSaver = androidx.compose.runtime.saveable.listSaver<List<BillItem>, Any>(
    save = { it.flatMap { b -> listOf(b.description, b.amountPxmr) } },
    restore = { flat -> flat.chunked(2).map { (d, a) -> BillItem(d as String, a as Long) } },
)

@Composable
fun PosScreen() {
    val context = LocalContext.current
    // Keyed below, so a field that opened before the first price fetch stops
    // being unusable the moment one lands.
    val rateVersion by ContactStore.changes.collectAsState()
    var basket by rememberSaveable(stateSaver = BasketSaver) { mutableStateOf(listOf<BillItem>()) }
    var taxPxmr by rememberSaveable { mutableStateOf(0L) }
    var charging by rememberSaveable { mutableStateOf(false) }
    // Two registers, one till: itemised for the shop that rings up lines,
    // quick for the coffee cart that only ever needs a number. Both end in
    // the same card, the same bill, the same receipt — quick just bills one
    // line named "Sale", because a §16.13 bill must still add up.
    var quick by rememberSaveable { mutableStateOf(false) }
    var quickAmount by rememberSaveable { mutableStateOf("") }
    var quickFiat by rememberSaveable { mutableStateOf(Amounts.enterFiat(context)) }

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
        // The running total is the hero, like every other screen's number —
        // and at the same sixteen they all sit at. This was twenty-four while
        // the item row under it was sixteen, so the till's left edge stepped
        // in and out down the screen. The home screen had the same split and
        // the same answer.
        Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
            Spacer(Modifier.height(16.dp))
            Text(
                stringResource(R.string.pos_this_sale),
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
                ) { Text(stringResource(R.string.pos_items)) }
                SegmentedButton(
                    selected = quick, onClick = { quick = true },
                    shape = SegmentedButtonDefaults.itemShape(1, 2),
                    icon = {},
                ) { Text(stringResource(R.string.pos_quick_amount)) }
            }
            Spacer(Modifier.height(12.dp))
        }

        if (quick) {
            val rate = remember(rateVersion) { RateStore(context).cached()?.first }
            val cur = remember(rateVersion) { Amounts.currency(context) }
            Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    OutlinedTextField(
                        value = quickAmount,
                        onValueChange = {
                            quickAmount = it.filter { c -> Amounts.isNumberChar(c) }
                        },
                        label = {
                            Text(stringResource(R.string.pos_total_in, if (quickFiat) cur else "XMR"))
                        },
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
                // BigDecimal, like the itemised path a few lines down. A
                // Double here rounded the last piconero off a long figure and
                // wrapped a huge one into a plausible small one.
                val pxmr = Amounts.parse(quickAmount)?.let { v ->
                    val xmr = when {
                        quickFiat && rate != null && rate > 0 ->
                            v.divide(
                                java.math.BigDecimal.valueOf(rate), 12,
                                java.math.RoundingMode.DOWN,
                            )
                        quickFiat -> null
                        else -> v
                    }
                    xmr?.let { Amounts.toPxmr(it) }
                }?.takeIf { it > 0 }
                Button(
                    onClick = {
                        basket = listOf(BillItem(context.getString(R.string.pos_sale), pxmr!!))
                        taxPxmr = 0L
                        quickAmount = ""
                        charging = true
                    },
                    enabled = pxmr != null,
                    modifier = Modifier.fillMaxWidth().height(56.dp),
                ) {
                    Text(
                        pxmr?.let {
                            stringResource(R.string.pos_request_amount, Amounts.show(context, it).primary)
                        } ?: stringResource(R.string.pos_request_payment),
                        style = MaterialTheme.typography.labelLarge,
                    )
                }
                Spacer(Modifier.height(24.dp))
            }
            return@Column
        }

        // Tap what you sell; the typed line below is still there for
        // whatever is not on the menu.
        ItemPicker { name, pxmr -> basket = basket + BillItem(name, pxmr) }
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
                                maxLines = 2,
                                overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f),
                            )
                            AmountBoth(item.amountPxmr)
                            IconButton(
                                onClick = {
                                    basket = basket.filterIndexed { j, _ -> j != i }
                                    // The tax line belongs to the lines it was
                                    // worked out on. Left standing, an emptied
                                    // basket showed the old tax as the whole
                                    // sale, and a typed one rode into the next.
                                    if (basket.isEmpty()) taxPxmr = 0L
                                },
                                // 40dp, not 32: the explicit size overrides
                                // the 48dp the component reserves, and this
                                // removes a rung-up line mid-sale.
                                modifier = Modifier.size(40.dp),
                            ) {
                                Icon(Icons.Filled.Close, stringResource(R.string.pos_remove),
                                    Modifier.size(16.dp))
                            }
                        }
                    }
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                    if (org.ducatproject.ducat.Tax.enabled(context)) {
                        // A rate is set, so the figure is arithmetic the till
                        // does in front of the customer — not a field. The
                        // typed row below stays for phones with no rate: an
                        // occasional seller quoting one odd levy is a different
                        // person from a business with a standing percentage.
                        val subtotal = basket.sumOf { it.amountPxmr }
                        // Keyed on the rate too (Tax.set bumps the store):
                        // a rate changed in Settings mid-sale used to leave
                        // the old figure on the bill until the basket moved.
                        LaunchedEffect(subtotal, rateVersion) {
                            taxPxmr = org.ducatproject.ducat.Tax.on(context, subtotal)
                        }
                        Row(
                            Modifier.fillMaxWidth().padding(vertical = 8.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                stringResource(
                                    R.string.pos_tax_at,
                                    org.ducatproject.ducat.Tax.percentText(
                                        org.ducatproject.ducat.Tax.basisPoints(context),
                                    ),
                                ),
                                style = MaterialTheme.typography.bodyLarge,
                                modifier = Modifier.weight(1f),
                            )
                            Text(
                                Amounts.show(context, taxPxmr).primary,
                                style = MaterialTheme.typography.bodyLarge,
                            )
                        }
                    } else {
                        TaxRow(taxPxmr) { taxPxmr = it }
                    }
                }
            }

            Spacer(Modifier.height(8.dp))
            // A Monero fee is the sender's, paid to the network. On a bill it
            // would be charged twice: once in the total and again when the
            // customer's wallet builds the transaction. §16.13 has no field
            // for it for exactly this reason.
            Text(
                stringResource(R.string.pos_network_fee_note),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.outline,
                modifier = Modifier.padding(horizontal = 16.dp),
            )

            Spacer(Modifier.height(16.dp))
            Button(
                onClick = { charging = true },
                enabled = total > 0,
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp).height(56.dp),
            ) {
                Text(
                    stringResource(R.string.pos_charge_amount, Amounts.show(context, total).primary),
                    style = MaterialTheme.typography.labelLarge,
                )
            }
            Spacer(Modifier.height(24.dp))
        } else {
            Spacer(Modifier.height(12.dp))
            Text(
                stringResource(R.string.pos_add_first_item),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp),
            )
        }
    }
}

/**
 * Both units on one line, the way this till reads them.
 *
 * Was hardcoded to XMR on top with "the fiat quietly under it", which stopped
 * being true the moment the local currency became the default: the second line
 * is [Shown.secondary], so the line read the same piconero figure twice and
 * never once said what the shop actually charges.
 */
@Composable
internal fun AmountBoth(pxmr: Long) {
    val context = LocalContext.current
    val shown = Amounts.show(context, pxmr)
    Column(horizontalAlignment = Alignment.End) {
        Text(
            shown.primary,
            style = MaterialTheme.typography.bodyMedium,
            fontFamily = FontFamily.Monospace,
        )
        shown.secondary?.let {
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
    // Saveable like the basket above it: the half-typed line is the sale's
    // newest work, and rotation happens exactly when a hand is busy.
    var desc by rememberSaveable { mutableStateOf("") }
    var amount by rememberSaveable { mutableStateOf("") }
    val rateVersion by ContactStore.changes.collectAsState()
    var fiat by rememberSaveable { mutableStateOf(Amounts.enterFiat(context)) }
    val rate = remember(rateVersion) { RateStore(context).cached()?.first }
    val cur = remember(rateVersion) { Amounts.currency(context) }

    val pxmr: Long? = remember(amount, fiat, rate) {
        val v = moneyText(amount).toBigDecimalOrNull() ?: return@remember null
        val xmr = if (fiat) {
            if (rate == null || rate <= 0) return@remember null
            v.divide(BigDecimal.valueOf(rate), 12, java.math.RoundingMode.DOWN)
        } else v
        Amounts.toPxmr(xmr)?.takeIf { it > 0 }
    }

    // Top-aligned, not centred: the counter under the item field makes
    // that composable taller than its neighbours, and centring unequal
    // heights is exactly how the two boxes ended up drawn at different
    // levels. With the tops pinned the boxes align; the buttons are
    // padded by hand to sit on the 56dp box's centre line.
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        verticalAlignment = Alignment.Top,
    ) {
        OutlinedTextField(
            value = desc,
            onValueChange = { if (it.length <= 64) desc = it },
            label = { Text(stringResource(R.string.pos_item)) },
            supportingText = { CharCounter(desc.length, 64) },
            singleLine = true,
            modifier = Modifier.weight(1.5f),
        )
        Spacer(Modifier.width(8.dp))
        OutlinedTextField(
            value = amount,
            onValueChange = { amount = it.filter { c -> Amounts.isNumberChar(c) } },
            label = { Text(if (fiat) cur else "XMR") },
            singleLine = true,
            keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                keyboardType = androidx.compose.ui.text.input.KeyboardType.Decimal,
            ),
            modifier = Modifier.weight(1f),
        )
        if (rate != null) {
            TextButton(
                // Convert what is typed rather than throwing it away, the way
                // the hail sheet already does. Tapping the unit is a question
                // about the same amount, not a decision to start again.
                onClick = {
                    val p = pxmr
                    fiat = !fiat
                    amount = p?.let { pxmrToField(it, fiat, rate) } ?: ""
                },
                contentPadding = PaddingValues(horizontal = 6.dp),
                modifier = Modifier.padding(top = 8.dp),
            ) {
                Text(
                    stringResource(R.string.pos_in_unit, if (fiat) "XMR" else cur),
                    style = MaterialTheme.typography.labelMedium,
                )
            }
        }
        FilledIconButton(
            onClick = { onAdd(desc.trim(), pxmr!!); desc = ""; amount = "" },
            enabled = desc.isNotBlank() && pxmr != null,
            modifier = Modifier.padding(top = 8.dp),
        ) { Icon(Icons.Filled.Add, stringResource(R.string.pos_add_line)) }
    }
}

@Composable
private fun TaxRow(taxPxmr: Long, onSet: (Long) -> Unit) {
    val context = LocalContext.current
    val rateVersion by ContactStore.changes.collectAsState()
    val rate = remember(rateVersion) { RateStore(context).cached()?.first }
    val cur = remember(rateVersion) { Amounts.currency(context) }
    var fiat by rememberSaveable { mutableStateOf(Amounts.enterFiat(context)) }
    // In the unit the field is showing, and never through `formatXmr`, which
    // localises its digits: a tax set, then reached again by switching tabs,
    // re-seeded this field from the stored piconero — as XMR under a label
    // reading USD, in Persian numerals on a Persian phone. Both halves of that
    // then came back out through the parser as the wrong number.
    var text by rememberSaveable {
        mutableStateOf(if (taxPxmr > 0) pxmrToField(taxPxmr, fiat, rate) else "")
    }

    /** Whatever unit the field is showing, as piconero. */
    fun toPxmr(s: String): Long {
        val v = Amounts.parse(s) ?: return 0L
        val xmr = if (fiat) {
            if (rate == null || rate <= 0) return 0L
            v.divide(BigDecimal.valueOf(rate), 12, java.math.RoundingMode.DOWN)
        } else {
            v
        }
        return Amounts.toPxmr(xmr) ?: 0L
    }
    Row(
        Modifier.fillMaxWidth().padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(stringResource(R.string.pos_tax), style = MaterialTheme.typography.bodyLarge)
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
                text = it.filter { c -> Amounts.isNumberChar(c) }
                // BigDecimal, not Double. `2.01 * 1e12` truncates to a
                // piconero short, and a customer holding an itemised bill
                // that reads 2.009999999999 tax is being shown arithmetic,
                // not a price.
                onSet(toPxmr(text))
            },
            label = { Text(if (fiat) cur else "XMR") },
            placeholder = { Text(Amounts.count(0)) },
            singleLine = true,
            keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                keyboardType = androidx.compose.ui.text.input.KeyboardType.Decimal,
            ),
            modifier = Modifier.width(150.dp),
        )
        if (rate != null) {
            TextButton(
                onClick = {
                    val p = toPxmr(text)
                    fiat = !fiat
                    text = if (p > 0) pxmrToField(p, fiat, rate) else ""
                    onSet(p)
                },
                contentPadding = PaddingValues(horizontal = 6.dp),
            ) {
                Text(
                    stringResource(R.string.pos_in_unit, if (fiat) "XMR" else cur),
                    style = MaterialTheme.typography.labelMedium,
                )
            }
        }
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
    // The card survives recreation: reissuing on rotation would put a fresh QR
    // on screen while the customer is mid-scan of the old one — their claim
    // would answer a card nobody is watching, and the bill would never send.
    var cardUri by rememberSaveable { mutableStateOf<String?>(null) }
    var cardInbox by rememberSaveable { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    // The customer as a persona hex — Contact does not fit a Bundle, the hex
    // does, and the store is the source of truth for the rest of them anyway.
    var customerHex by rememberSaveable { mutableStateOf<String?>(null) }
    var saleTabId by rememberSaveable { mutableStateOf<String?>(null) }
    // The tab opened for this sale before its bill has gone. One per sale,
    // however many tries the bill takes: the billing loop used to open a
    // fresh tab on every attempt, and a node that was down for a minute left
    // thirty "settled" tabs in the store for one coffee — each a live claim
    // on the next payment of that size from that customer, none of them
    // withdrawn by leaving the screen, which only knew about the last.
    var pendingTabId by rememberSaveable { mutableStateOf<String?>(null) }
    // When the sale went up, so its own bill can be told from an older one
    // to the same customer for the same things.
    val presentedAt = rememberSaveable { System.currentTimeMillis() }
    val customer: Contact? = remember(customerHex, version) {
        customerHex?.let { h -> ContactStore(context).all().firstOrNull { it.personaHex == h } }
    }

    // The stage is *derived* from the settlement store, which the poller
    // drives — so a sale marked paid while this screen was backgrounded shows
    // paid the moment it returns, and the receipt logic lives in exactly one
    // place (TabStore.reconcile) for every vendor mode.
    val saleTab = remember(version, saleTabId) { saleTabId?.let { TabStore(context).get(it) } }
    val stage = when {
        saleTabId != null -> when {
            saleTab?.state == "paid" -> Sale.Paid
            saleTab?.seenTx != null -> Sale.Seen
            else -> Sale.Billed
        }
        else -> Sale.Waiting
    }
    // Whether the receipt actually left. "Receipt sent" used to be read
    // off the paid state alone, over a send the reconciler had logged as
    // failed and forgotten; the tab now owes it, and the poll sends it.
    val receiptOwed = saleTab?.wordSeq == RunningTab.WORD_UNSENT

    val scope = rememberCoroutineScope()
    var leaving by remember { mutableStateOf(false) }
    // Leaving a billed sale must withdraw the bill, not just the screen.
    // The settled record would otherwise sit in the store forever, and a
    // later unrelated payment of the same amount would match it — a
    // receipt fired into a dead sale's thread.
    //
    // And the screen waits for the word to go. It used to fire the retract
    // and leave in the same tap, which was the same bug on a timer: a node
    // that refused the send failed inside a coroutine nobody was watching
    // (or one the departing screen had already cancelled), TabStore.close
    // put the tab back to "settled" as it must, and the till returned to
    // its basket with the bill live on the customer's phone and the claim
    // live in the store — now with no screen that knew about either. The
    // Bar Tab's cancel stays until the word is out or says why it is not;
    // this does the same, and the buttons are the retry.
    fun leave(then: () -> Unit) {
        val billed = saleTabId
        val pending = pendingTabId
        if (billed == null && pending == null) { then(); return }
        if (leaving) return
        leaving = true
        error = null
        scope.launch {
            val r = withContext(Dispatchers.IO) {
                runCatching {
                    val store = TabStore(context)
                    if (billed != null) {
                        store.get(billed)
                            ?.takeIf { it.state == "settled" }
                            // The retract path: the customer's Review button
                            // greys out by itself when the bill can be named.
                            ?.let { cancelTabWithRetract(context, store, it) }
                    } else {
                        // Opened, never billed — unless one attempt's bill did
                        // leave and only the bookkeeping after it failed, in
                        // which case the thread holds it and it is withdrawn
                        // like any other. Otherwise there is nothing on their
                        // phone to withdraw, and a "your bill was cancelled" for
                        // a bill they never saw would only puzzle them: the tab
                        // is closed quietly so no later payment can match it.
                        val tab = store.get(pending!!) ?: return@runCatching
                        val went = ContactStore(context).thread(tab.personaHex).any {
                            it.outgoing && it.kind == 1 && it.amountPxmr == tab.settledTotal
                        }
                        if (went) {
                            cancelTabWithRetract(context, store, tab)
                        } else {
                            store.mutate(tab.id) { it.copy(state = "cancelled") }
                        }
                    }
                }
            }
            leaving = false
            r.onSuccess { then() }
                .onFailure {
                    error = context.getString(
                        R.string.pos_error_bill_not_withdrawn, moneyFailure(context, it),
                    )
                    DucatLog.e(TAG, "withdraw: ${it.message}")
                }
        }
    }

    // Back is the on-screen Back button, retraction included.
    //
    // It used to quit the app from a sale already presented to a customer;
    // then it returned to the basket and did only that, which is the same bug
    // wearing a quieter face. The button beside the QR withdraws the bill
    // first, so leaving by gesture left a settled tab nothing would ever close
    // — waiting to be matched by the next unrelated payment that happens to
    // come to the same figure — and a live "Review payment" on the customer's
    // phone for a sale the till had already forgotten.
    //
    // Sighted and paid sales are exempt for the reason the Cancel button
    // gives below: the money is in flight, and retracting now orphans it —
    // and for those the gesture is the "New sale" button, not the Back one
    // the screen no longer shows. Returning to the basket with the sold
    // items still in it rang the next customer up for the last one's
    // coffee.
    BackHandler {
        if (stage == Sale.Waiting || stage == Sale.Billed) leave(onBack)
        else onDone()
    }

    // While the code is up, a tap offers the same sale card.
    DisposableEffect(cardUri) {
        org.ducatproject.ducat.nfc.Tap.offered = cardUri
        onDispose { org.ducatproject.ducat.nfc.Tap.offered = null }
    }

    // A card per sale, marked as one: a "sale" card never auto-reissues, and
    // this flow waits for *its* claimant — a profile-code scan mid-sale must
    // not be billed as the customer. Issued once per sale, not per screen
    // instance: a restored screen already holds its card and keeps it.
    LaunchedEffect(Unit) {
        if (cardUri != null) return@LaunchedEffect
        // Until it is cut: the till's first sale is rung before the node
        // has attached, and one refused cut used to leave the sale wearing
        // "offline" until it was abandoned and rung again.
        val card = issueCardPatiently(context, 60uL * 60uL * 2uL, "sale") { error = it }
        error = null
        cardUri = card.uri; cardInbox = card.inboxKey
    }

    // Scan → bill. The claim bound to this card makes them a contact; the
    // bill lands in the new conversation, itemised, destination inside it,
    // and the sale becomes a settlement the poller watches.
    LaunchedEffect(cardInbox) {
        val inbox = cardInbox ?: return@LaunchedEffect
        // Keyed on the tab, not the contact: the tab is the work this loop
        // exists to do, and a restored screen whose sale is already billed
        // (saleTabId saved) must not bill it twice.
        while (saleTabId == null) {
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
                    // A bill this screen already sent and then lost track
                    // of. The saved state is written when the activity
                    // stops — a locked screen, a call — and this loop keeps
                    // running after that, so the bill can go out with no
                    // bundle to record it; if Android then reclaims the
                    // process, the restored screen holds the card and the
                    // claimant and none of the ids, and billed the same
                    // customer the same coffee again. The tab is in the
                    // store whatever the bundle says: this sale's own,
                    // billed and unpaid, opened since the sale went up.
                    store.all().firstOrNull {
                        it.origin == "pos" && it.personaHex == fresh.personaHex &&
                            it.state == "settled" && it.openedAt >= presentedAt &&
                            it.lines == items && it.taxPxmr == taxPxmr
                    }?.let { return@runCatching it.id }
                    // The same tab on every try; a retry re-sends the bill
                    // through it rather than opening another.
                    val id = pendingTabId?.takeIf { store.get(it) != null }
                        ?: store.open(fresh.personaHex, "pos").id.also { pendingTabId = it }
                    val filled = store.mutate(id) {
                        it.copy(lines = items, taxPxmr = taxPxmr)
                    }!!
                    store.settle(filled)
                    id
                }.onSuccess { id ->
                    customerHex = fresh.personaHex
                    saleTabId = id
                    pendingTabId = null
                    error = null
                    DucatLog.i(TAG, "billed ${fresh.displayName()} ${formatXmr(totalPxmr)} XMR")
                }.onFailure {
                    // The frame was localised and the reason was not, so a
                    // till in Bangkok read "could not send the bill:
                    // InterfaceError(...)". Billing reaches the same node as
                    // everything else and fails the same way.
                    error = context.getString(
                        R.string.pos_error_bill_not_sent, moneyFailure(context, it),
                    )
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
                            maxLines = 2, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                            style = MaterialTheme.typography.bodyMedium)
                        Text(Amounts.show(context, i.amountPxmr).primary,
                            style = MaterialTheme.typography.bodyMedium,
                            fontFamily = FontFamily.Monospace)
                    }
                }
                taxPxmr?.let {
                    Row(Modifier.fillMaxWidth().padding(vertical = 2.dp)) {
                        Text(stringResource(R.string.pos_tax), Modifier.weight(1f),
                            style = MaterialTheme.typography.bodyMedium)
                        Text(Amounts.show(context, it).primary,
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
                    CatSpinner(Modifier.size(40.dp), tint = MaterialTheme.colorScheme.primary)
                    Spacer(Modifier.height(10.dp))
                    Text(stringResource(R.string.pos_getting_code_ready),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                cardUri != null -> {
                    QrBlock(cardUri!!)
                    Spacer(Modifier.height(10.dp))
                    Text(
                        stringResource(R.string.pos_scan_hint),
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
                            stringResource(R.string.pos_tap_hint),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.outline,
                        )
                    }
                }
                else -> {}
            }
            Sale.Seen -> {
                Text(
                    stringResource(R.string.pos_payment_seen),
                    style = MaterialTheme.typography.headlineSmall,
                    color = MaterialTheme.ducat.changePending,
                )
                Spacer(Modifier.height(4.dp))
                Text(
                    stringResource(R.string.pos_payment_settling),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
                Spacer(Modifier.height(10.dp))
                CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
            }
            Sale.Billed -> {
                Text(
                    stringResource(
                        R.string.pos_bill_sent_to,
                        customer?.let { isolate(it.displayName()) } ?: stringResource(R.string.pos_the_customer),
                    ),
                    style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(6.dp))
                Text(
                    stringResource(R.string.pos_billed_hint),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
                Spacer(Modifier.height(10.dp))
                CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
            }
            Sale.Paid -> {
                Text(stringResource(R.string.pos_paid), style = MaterialTheme.typography.headlineMedium,
                    color = MaterialTheme.ducat.settled)
                Spacer(Modifier.height(4.dp))
                Text(
                    if (receiptOwed) stringResource(R.string.pos_receipt_pending)
                    else stringResource(
                        R.string.pos_receipt_sent_to,
                        customer?.let { isolate(it.displayName()) } ?: stringResource(R.string.pos_the_customer),
                    ),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
            }
        }

        error?.let {
            Spacer(Modifier.height(10.dp))
            Text(it, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall)
        }

        Spacer(Modifier.height(20.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            if (stage == Sale.Waiting || stage == Sale.Billed) {
                OutlinedButton(
                    onClick = { leave(onBack) },
                    enabled = !leaving,
                    modifier = Modifier.weight(1f).height(48.dp),
                ) {
                    if (leaving) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                    else Text(stringResource(R.string.pos_back))
                }
            }
            Button(
                onClick = {
                    // Once a payment is sighted, the money is in flight and
                    // cancelling the bill would orphan it — the way out is
                    // letting it settle.
                    if (stage == Sale.Waiting || stage == Sale.Billed) leave(onDone)
                    else onDone()
                },
                enabled = !leaving,
                modifier = Modifier.weight(1f).height(48.dp),
            ) {
                Text(
                    when (stage) {
                        Sale.Paid -> stringResource(R.string.pos_new_sale)
                        Sale.Seen -> stringResource(R.string.pos_new_sale_settles)
                        else -> stringResource(R.string.pos_cancel_sale)
                    },
                    maxLines = 1, softWrap = false,
                    style = MaterialTheme.typography.labelMedium,
                )
            }
        }
        Spacer(Modifier.height(24.dp))
    }
}
