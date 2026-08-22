package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.Catalogue
import org.ducatproject.ducat.formatXmr
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.R

/**
 * What this till sells, and what each thing costs.
 *
 * One list, shared by every mode that rings something up: a market stall's
 * till and a bar's tab are the same shop with the same prices, and making
 * somebody type their menu twice would be the kind of thing that stops them
 * using either.
 */
@Composable
fun ItemsScreen() {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val items = remember(version) { Catalogue.all(context) }
    var name by rememberSaveable { mutableStateOf("") }
    var price by rememberSaveable { mutableStateOf("") }
    val currency = remember(version) { Amounts.currency(context) }

    Column(Modifier.fillMaxSize().padding(16.dp)) {
        Text(stringResource(R.string.items_title), style = MaterialTheme.typography.titleLarge)
        Spacer(Modifier.height(4.dp))
        Text(
            stringResource(R.string.items_note, currency),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(12.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(
                value = name,
                onValueChange = { if (it.length <= 40) name = it },
                label = { Text(stringResource(R.string.items_name)) },
                supportingText = { CharCounter(name.length, 40) },
                singleLine = true,
                modifier = Modifier.weight(1.6f),
            )
            Spacer(Modifier.width(8.dp))
            OutlinedTextField(
                value = price,
                onValueChange = { price = it.filter { c -> Amounts.isNumberChar(c) } },
                label = { Text(currency) },
                singleLine = true,
                keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                    keyboardType = androidx.compose.ui.text.input.KeyboardType.Decimal,
                ),
                modifier = Modifier.weight(1f),
            )
            Spacer(Modifier.width(8.dp))
            Button(
                onClick = {
                    Catalogue.put(context, Catalogue.draft(context, name.trim(), price.trim()))
                    name = ""; price = ""
                },
                enabled = name.isNotBlank() && price.isNotBlank(),
                modifier = Modifier.height(52.dp),
            ) { Text(stringResource(R.string.items_add)) }
        }
        Spacer(Modifier.height(12.dp))
        // else, not an early return.
        //
        // `return` here is a *non-local* return out of ItemsScreen from inside
        // Column's inline lambda, so the groups Column had opened were never
        // closed. Compose does not survive that: the composer unwinds to a
        // marker that is no longer on its stack and the app dies with
        // ArrayIndexOutOfBoundsException in IntStack.peek2, from a frame with
        // nothing of ours in it.
        //
        // It fired on the one path anybody takes first — a till with no items
        // yet, which is every till until somebody types one. Opening Items on
        // a fresh kiosk crashed the app every time.
        if (items.isEmpty()) {
            Text(
                stringResource(R.string.items_none),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        } else LazyColumn(Modifier.fillMaxSize()) {
            items(items) { item ->
                ListItem(
                    // Transparent, as everywhere else: a ListItem defaults to
                    // `surface`, which is not the page it sits on.
                    colors = ListItemDefaults.colors(
                        containerColor = androidx.compose.ui.graphics.Color.Transparent,
                    ),
                    headlineContent = {
                        Text(
                            item.name,
                            color = if (item.soldOut) {
                                MaterialTheme.colorScheme.onSurfaceVariant
                            } else {
                                MaterialTheme.colorScheme.onSurface
                            },
                        )
                    },
                    supportingContent = { Text(ItemPriceLine(item)) },
                    trailingContent = {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            // Off today, back tomorrow — which is not the same
                            // as deleting it and typing the price again in the
                            // morning.
                            TextButton(
                                onClick = {
                                    Catalogue.put(context, item.copy(soldOut = !item.soldOut))
                                },
                            ) {
                                Text(
                                    stringResource(
                                        if (item.soldOut) R.string.items_back_on
                                        else R.string.items_sold_out,
                                    ),
                                )
                            }
                            IconButton(onClick = { Catalogue.remove(context, item.id) }) {
                                Icon(Icons.Filled.Delete, stringResource(R.string.items_remove))
                            }
                        }
                    },
                )
                HorizontalDivider()
            }
        }
    }
}

/** "3.20 GBP · 0.001320 XMR", or why it cannot be rung up. */
@Composable
private fun ItemPriceLine(item: Catalogue.Item): String {
    val context = LocalContext.current
    val priced = remember(item, ContactStore.changes.collectAsState().value) {
        Catalogue.price(context, item)
    }
    val shown = "${item.price} ${item.currency}"
    val snag = (priced.exceptionOrNull() as? Catalogue.SnagException)?.snag
    return when {
        // The XMR side, always — not `show().primary`, which follows the
        // reader's own fiat preference and so rendered a till priced in
        // dollars as "2.50 USD · USD 2.50". The whole point of the line is
        // what the typed price converts to at today's rate; saying it back in
        // the currency it was typed in is not an answer to that.
        priced.isSuccess ->
            "$shown · ${formatXmr(priced.getOrThrow().pxmr)} XMR"
        snag == Catalogue.Snag.WrongCurrency ->
            "$shown · ${stringResource(R.string.items_other_currency)}"
        snag == Catalogue.Snag.NoRate -> "$shown · ${stringResource(R.string.items_no_rate)}"
        else -> "$shown · ${stringResource(R.string.items_bad_price)}"
    }
}

/**
 * The menu, as buttons — the whole point of the list above.
 *
 * Nothing when the catalogue is empty: a till that has never had items set up
 * should look exactly as it always did, not carry a permanently empty shelf.
 */
@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
fun ItemPicker(onPick: (String, Long) -> Unit) {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    // Priced once per change, not once per frame. Every one of these reads the
    // rate out of an encrypted store; doing it inside the loop below meant a
    // decrypt per item per recomposition, on the screen a queue is watching.
    val priced = remember(version) {
        Catalogue.sellable(context).map { it to Catalogue.price(context, it) }
    }
    if (priced.isEmpty()) return
    // How old the rate behind these prices is, said once rather than on every
    // button: a till with no signal still sells, at the last rate it saw, and
    // the person holding it should know that is what is happening.
    //
    // Derived, not accumulated. This used to be a `var` raised inside the loop
    // — a state write during composition, which both invites a recomposition
    // loop and only ever went up: once a bad afternoon had pushed it to four
    // hours it stayed there, and the screen went on apologising for a rate it
    // had long since refreshed.
    val stale = remember(priced) {
        priced.mapNotNull { (_, p) -> p.getOrNull()?.staleSecs }.maxOrNull() ?: 0L
    }
    FlowRow(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        priced.forEach { (item, result) ->
            val p = result.getOrNull()
            AssistChip(
                onClick = { p?.let { onPick(item.name, it.pxmr) } },
                enabled = p != null,
                // With the currency: this is the face a customer reads, and
                // "Croissant · 2.50" does not say 2.50 of what.
                label = { Text("${item.name} · ${item.price} ${item.currency}") },
            )
        }
    }
    // Every button dead and nothing saying why is the worst version of this
    // screen, and it is the one a stall meets on its first morning: prices are
    // in pounds, converting them needs a rate, and a phone that has not
    // reached the network yet has none. The chips disable themselves
    // correctly; without this the seller is left tapping them.
    if (priced.all { (_, p) -> (p.exceptionOrNull() as? Catalogue.SnagException)?.snag == Catalogue.Snag.NoRate }) {
        Text(
            stringResource(R.string.items_picker_no_rate),
            Modifier.padding(horizontal = 16.dp),
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.error,
        )
    }
    if (stale > STALE_RATE_SECS) {
        Text(
            stringResource(R.string.items_rate_stale, humanDuration(context, stale)),
            Modifier.padding(horizontal = 16.dp),
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

/** Past this, the rate behind a price is worth mentioning out loud. */
private const val STALE_RATE_SECS = 60L * 60
