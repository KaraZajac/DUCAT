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
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.BillItem
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.RateStore
import org.ducatproject.ducat.WalletStore
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
            Spacer(Modifier.height(16.dp))
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
        val v = amount.toBigDecimalOrNull() ?: return@remember null
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
            onValueChange = { amount = it.filter { c -> c.isDigit() || c == '.' } },
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
                text = it.filter { c -> c.isDigit() || c == '.' }
                onSet(text.toDoubleOrNull()?.let { v -> (v * 1e12).toLong() } ?: 0L)
            },
            label = { Text("XMR") },
            placeholder = { Text("0") },
            singleLine = true,
            modifier = Modifier.width(150.dp),
        )
    }
}

/** Where the sale stands, so the screen can say it in one word. */
private enum class Sale { Waiting, Billed, Paid }

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
    var stage by remember { mutableStateOf(Sale.Waiting) }
    var cardUri by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var customer by remember { mutableStateOf<Contact?>(null) }

    // A card per sale. One card reused across customers would put the whole
    // day in one conversation and reuse the address — §16.13's linkage.
    LaunchedEffect(Unit) {
        val r = withContext(Dispatchers.IO) {
            runCatching {
                Mailbox.issueCard(context, MyProfile(context).name(), 60uL * 60uL * 2uL)
            }
        }
        r.onSuccess { cardUri = it.uri }
            .onFailure {
                error = it.message ?: "could not publish the code"
                DucatLog.e(TAG, "card: ${it.message}")
            }
    }

    // Scan → bill. The claim makes them a contact; the bill is the first thing
    // in the conversation, itemised, with the destination inside it.
    LaunchedEffect(cardUri) {
        if (cardUri == null) return@LaunchedEffect
        val before = ContactStore(context).all().map { it.personaHex }.toSet()
        while (customer == null) {
            delay(2_000)
            val fresh = withContext(Dispatchers.IO) {
                runCatching {
                    Mailbox.collectClaims(context)
                    ContactStore(context).all().firstOrNull { it.personaHex !in before }
                }.getOrNull()
            } ?: continue
            withContext(Dispatchers.IO) {
                runCatching {
                    Mailbox.send(
                        context, fresh, "Your bill",
                        PersonaStore(context).personaHex(),
                        kind = 1, amountPxmr = totalPxmr,
                        payto = WalletStore(context).address(),
                        items = items, taxPxmr = taxPxmr,
                    )
                }.onSuccess {
                    customer = fresh
                    stage = Sale.Billed
                    DucatLog.i(TAG, "billed ${fresh.displayName()} ${formatXmr(totalPxmr)} XMR")
                }.onFailure {
                    error = "They connected, but the bill did not send: ${it.message}"
                    DucatLog.e(TAG, "bill: ${it.message}")
                }
            }
        }
    }

    // Payment → receipt, automatically. The poller reads the chain; when an
    // output of exactly the total appears, that is this sale settling, and the
    // receipt goes into the same thread pointing at the transaction. Exact
    // match only: a till with two customers mid-sale must not thank the wrong
    // one, so anything else stays unmatched and shows up in Activity instead.
    LaunchedEffect(stage) {
        if (stage != Sale.Billed) return@LaunchedEffect
        val already = WalletStore(context).entries().map { it.keyImage }.toSet()
        while (stage == Sale.Billed) {
            delay(3_000)
            val paid = withContext(Dispatchers.IO) {
                WalletStore(context).entries()
                    .firstOrNull { it.keyImage !in already && it.amountPxmr == totalPxmr }
            } ?: continue
            withContext(Dispatchers.IO) {
                runCatching {
                    Mailbox.send(
                        context, customer!!, "Receipt — thank you",
                        PersonaStore(context).personaHex(),
                        kind = 3, amountPxmr = totalPxmr,
                        items = items, taxPxmr = taxPxmr,
                        txidHex = paid.txHashHex.ifEmpty { null },
                    )
                }.onSuccess { DucatLog.i(TAG, "receipt sent for ${formatXmr(totalPxmr)} XMR") }
                    .onFailure { DucatLog.w(TAG, "receipt: ${it.message}") }
            }
            stage = Sale.Paid
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
                            "Tap to pay is not built yet",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.outline,
                        )
                    }
                }
                else -> {}
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
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            if (stage != Sale.Paid) {
                OutlinedButton(onClick = onBack, modifier = Modifier.weight(1f).height(48.dp)) {
                    Text("Back")
                }
            }
            Button(onClick = onDone, modifier = Modifier.weight(1f).height(48.dp)) {
                Text(if (stage == Sale.Paid) "New sale" else "Cancel sale")
            }
        }
        Spacer(Modifier.height(24.dp))
    }
}
