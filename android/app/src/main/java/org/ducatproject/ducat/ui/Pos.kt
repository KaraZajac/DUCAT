package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
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
import org.ducatproject.ducat.NameStore
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.WalletStore
import org.ducatproject.ducat.formatXmr

private const val TAG = "POS"

/**
 * Point of sale: ring up a total, present it, take the payment.
 *
 * The two ways to present it are **not** cosmetic variants of one another, and
 * the toggle says so rather than hiding it:
 *
 * - **DUCAT** hands over a contact card. Claiming it creates a conversation, so
 *   the bill arrives as an itemised request in a thread that survives the
 *   customer walking away — and the receipt can follow it (§16.13). It also
 *   means the vendor never publishes a reusable address: the request names a
 *   destination and nothing has to be stored against anyone.
 * - **Monero** is a plain `monero:` URI. Any wallet on earth can pay it, and it
 *   buys none of the above: no thread, no receipt, no itemisation, and the
 *   address on screen is the same one every customer sees, which is a public
 *   ledger entry linking all of them.
 *
 * The fallback exists because a till that only works for people who already run
 * this app is a till nobody installs. Its cost is stated on the screen where the
 * choice is made.
 */
@Composable
fun PosScreen() {
    var basket by remember { mutableStateOf(listOf<BillItem>()) }
    var taxPxmr by remember { mutableStateOf(0L) }
    var charging by remember { mutableStateOf(false) }

    val subtotal = basket.sumOf { it.amountPxmr }
    val total = subtotal + taxPxmr

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

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp)) {
        Text(
            "Ring up a sale, then present it. A DUCAT customer gets a conversation " +
                "and an itemised bill; anyone else can pay a plain Monero code.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(16.dp))

        AddLine { d, a -> basket = basket + BillItem(d, a) }

        if (basket.isNotEmpty()) {
            Spacer(Modifier.height(16.dp))
            Card(Modifier.fillMaxWidth()) {
                Column(Modifier.padding(14.dp)) {
                    basket.forEachIndexed { i, it ->
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(it.description, Modifier.weight(1f))
                            Text(formatXmr(it.amountPxmr), fontFamily = FontFamily.Monospace)
                            IconButton(onClick = {
                                basket = basket.filterIndexed { j, _ -> j != i }
                            }) { Icon(Icons.Filled.Close, "Remove", Modifier.size(16.dp)) }
                        }
                    }
                    HorizontalDivider()
                    Spacer(Modifier.height(8.dp))
                    TaxRow(taxPxmr) { taxPxmr = it }
                    Spacer(Modifier.height(8.dp))
                    HorizontalDivider()
                    Spacer(Modifier.height(8.dp))
                    Row {
                        Text("Total", fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
                        Column(horizontalAlignment = Alignment.End) {
                            Text(
                                "${formatXmr(total)} XMR",
                                fontFamily = FontFamily.Monospace,
                                fontWeight = FontWeight.SemiBold,
                            )
                            Amounts.show(LocalContext.current, total).secondary?.let {
                                Text(
                                    it,
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.outline,
                                )
                            }
                        }
                    }
                }
            }

            Spacer(Modifier.height(8.dp))
            // Stated where the total is, because it is the number a vendor is
            // about to charge. A Monero fee is paid by the *sender* to the
            // network; adding it to a bill charges it twice, once here and again
            // when the customer's wallet builds the transaction. §16.13 has no
            // field for it for exactly this reason.
            Text(
                "The network fee is not yours to bill — the customer's wallet pays it " +
                    "on top of this, to the network, not to you.",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.outline,
            )

            Spacer(Modifier.height(16.dp))
            Button(
                onClick = { charging = true },
                enabled = total > 0,
                modifier = Modifier.fillMaxWidth().height(52.dp),
            ) { Text("Charge ${formatXmr(total)} XMR") }
        }
    }
}

@Composable
private fun AddLine(onAdd: (String, Long) -> Unit) {
    var desc by remember { mutableStateOf("") }
    var amount by remember { mutableStateOf("") }
    val pxmr = amount.toDoubleOrNull()?.let { (it * 1e12).toLong() }?.takeIf { it > 0 }

    Row(verticalAlignment = Alignment.CenterVertically) {
        OutlinedTextField(
            value = desc,
            onValueChange = { if (it.length <= 64) desc = it },
            label = { Text("Item") },
            singleLine = true,
            modifier = Modifier.weight(1.4f),
        )
        Spacer(Modifier.width(8.dp))
        OutlinedTextField(
            value = amount,
            onValueChange = { amount = it.filter { c -> c.isDigit() || c == '.' } },
            label = { Text("XMR") },
            singleLine = true,
            modifier = Modifier.weight(1f),
        )
        IconButton(
            onClick = { onAdd(desc.trim(), pxmr!!); desc = ""; amount = "" },
            enabled = desc.isNotBlank() && pxmr != null,
        ) { Icon(Icons.Filled.Add, "Add line") }
    }
}

@Composable
private fun TaxRow(taxPxmr: Long, onSet: (Long) -> Unit) {
    var text by remember { mutableStateOf(if (taxPxmr > 0) formatXmr(taxPxmr) else "") }
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text("Tax", Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
        OutlinedTextField(
            value = text,
            onValueChange = {
                text = it.filter { c -> c.isDigit() || c == '.' }
                onSet(text.toDoubleOrNull()?.let { v -> (v * 1e12).toLong() } ?: 0L)
            },
            placeholder = { Text("0") },
            singleLine = true,
            modifier = Modifier.width(140.dp),
        )
    }
}

/** How the total is handed to the customer. */
private enum class Present { Ducat, Monero }

@Composable
private fun PresentScreen(
    items: List<BillItem>,
    taxPxmr: Long?,
    totalPxmr: Long,
    onDone: () -> Unit,
    onBack: () -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var mode by remember { mutableStateOf(Present.Ducat) }
    var cardUri by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var billedTo by remember { mutableStateOf<Contact?>(null) }
    val address = remember { WalletStore(context).address() }

    // A card per sale, not one reused across customers. Two reasons and both
    // matter at a till: a shared card would put every customer of the day in one
    // conversation, and a card is what carries the *destination*, so reusing it
    // reuses the address — which is the public-ledger linkage §16.13 refuses.
    LaunchedEffect(Unit) {
        busy = true
        val r = withContext(Dispatchers.IO) {
            runCatching {
                Mailbox.issueCard(context, NameStore(context).get(), 60uL * 60uL * 2uL)
            }
        }
        busy = false
        r.onSuccess { cardUri = it.uri }
            .onFailure {
                error = it.message ?: "could not publish a card"
                DucatLog.e(TAG, "card: ${it.message}")
            }
    }

    // Watch for the customer claiming it, then send them the bill. This is the
    // whole reason a till would prefer DUCAT to a payment code: what arrives is
    // a conversation, so the bill is itemised and the receipt has somewhere to
    // go afterwards.
    LaunchedEffect(cardUri) {
        if (cardUri == null) return@LaunchedEffect
        val before = ContactStore(context).all().map { it.personaHex }.toSet()
        while (billedTo == null) {
            delay(2_000)
            val fresh = withContext(Dispatchers.IO) {
                runCatching {
                    Mailbox.collectClaims(context)
                    ContactStore(context).all().firstOrNull { it.personaHex !in before }
                }.getOrNull()
            } ?: continue
            val sent = withContext(Dispatchers.IO) {
                runCatching {
                    Mailbox.send(
                        context, fresh,
                        "Your bill",
                        PersonaStore(context).personaHex(),
                        kind = 1,
                        amountPxmr = totalPxmr,
                        payto = WalletStore(context).address(),
                        items = items,
                        taxPxmr = taxPxmr,
                    )
                }
            }
            sent.onSuccess {
                billedTo = fresh
                DucatLog.i(TAG, "billed ${fresh.displayName()} ${formatXmr(totalPxmr)} XMR")
            }.onFailure {
                error = "They arrived, but the bill did not send: ${it.message}"
                DucatLog.e(TAG, "bill: ${it.message}")
            }
        }
    }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("${formatXmr(totalPxmr)} XMR", fontSize = 32.sp, fontWeight = FontWeight.Bold)
        Amounts.show(context, totalPxmr).secondary?.let {
            Text(it, style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Spacer(Modifier.height(16.dp))

        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
            SegmentedButton(
                selected = mode == Present.Ducat,
                onClick = { mode = Present.Ducat },
                shape = SegmentedButtonDefaults.itemShape(0, 2),
                modifier = Modifier.weight(1f),
            ) { Text("DUCAT", maxLines = 1, softWrap = false) }
            SegmentedButton(
                selected = mode == Present.Monero,
                onClick = { mode = Present.Monero },
                shape = SegmentedButtonDefaults.itemShape(1, 2),
                modifier = Modifier.weight(1f),
            ) { Text("Monero", maxLines = 1, softWrap = false) }
        }

        Spacer(Modifier.height(16.dp))

        when (mode) {
            Present.Ducat -> when {
                busy || cardUri == null -> Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    CircularProgressIndicator()
                    Spacer(Modifier.height(10.dp))
                    Text("Publishing the till's inbox…",
                        style = MaterialTheme.typography.bodySmall)
                }
                billedTo != null -> Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text("Bill sent to ${billedTo!!.displayName()}",
                        style = MaterialTheme.typography.titleMedium)
                    Spacer(Modifier.height(6.dp))
                    Text(
                        "It is a request, not a charge — they still confirm on their " +
                            "own device. When the payment lands you can send them a " +
                            "receipt from the conversation.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                else -> Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    QrBlock(cardUri!!)
                    Spacer(Modifier.height(12.dp))
                    Text("Waiting for a scan…", style = MaterialTheme.typography.bodySmall)
                    Spacer(Modifier.height(10.dp))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Filled.Nfc, null, Modifier.size(16.dp),
                            tint = MaterialTheme.colorScheme.outline)
                        Spacer(Modifier.width(6.dp))
                        // Named as missing rather than drawn as a spinner that
                        // will never resolve. §15's tap needs the card handed
                        // over the NFC link, and this app answers `6A82` to
                        // every select — the reader would sit there forever.
                        Text(
                            "Tap to pay is not built yet — scan the code",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.outline,
                        )
                    }
                }
            }

            Present.Monero -> Column(horizontalAlignment = Alignment.CenterHorizontally) {
                if (address == null) {
                    Text("No wallet yet — finish setup first.",
                        color = MaterialTheme.colorScheme.error)
                } else {
                    // `tx_amount` is the BIP-21-style parameter Monero wallets
                    // read, in XMR rather than piconero.
                    QrBlock("monero:$address?tx_amount=${formatXmr(totalPxmr)}")
                    Spacer(Modifier.height(12.dp))
                    Surface(
                        color = MaterialTheme.colorScheme.surfaceVariant,
                        shape = RoundedCornerShape(10.dp),
                    ) {
                        Text(
                            "Any wallet can pay this, and it buys none of what DUCAT " +
                                "does: no conversation, no itemised bill, no receipt. " +
                                "It also shows every customer the same address, which " +
                                "on a public ledger links all of them together.",
                            style = MaterialTheme.typography.bodySmall,
                            modifier = Modifier.padding(12.dp),
                        )
                    }
                }
            }
        }

        error?.let {
            Spacer(Modifier.height(12.dp))
            Text(it, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall)
        }

        Spacer(Modifier.height(24.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            OutlinedButton(onClick = onBack, modifier = Modifier.weight(1f)) { Text("Back") }
            Button(onClick = onDone, modifier = Modifier.weight(1f)) { Text("New sale") }
        }
    }
}
