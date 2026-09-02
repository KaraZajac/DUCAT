package org.ducatproject.ducat.ui

import android.content.Context
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDownward
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.automirrored.filled.HelpOutline
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Receipt
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Close
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.material.icons.filled.IosShare
import kotlinx.coroutines.launch
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Ledger
import org.ducatproject.ducat.NodeStore
import org.ducatproject.ducat.R
import org.ducatproject.ducat.Wallet

/**
 * What has actually happened, as transactions rather than outputs.
 *
 * The distinction is the whole screen. See [Ledger]: a wallet stores outputs,
 * and reading them out directly shows a send's change as income, hides the send
 * itself, and reports a total nobody can reconcile against the balance.
 *
 * Every row carries the balance *after* it, so the column adds up to the number
 * on Accounts. That is not decoration — it is the only way someone can check
 * that this screen and that one are describing the same money.
 */
@Composable
fun ActivityScreen() {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    // Off the main thread for the same reason as the home screen's recent
    // rows: building the ledger costs what the ledger is long, and a tab
    // that blocks composition while it reads is a frozen app, not a tab.
    var events by remember { mutableStateOf<List<Ledger.Event>>(emptyList()) }
    // openRequests walks every thread and re-walks each for its bills — the
    // same history-sized work as the ledger one line up, and it was left on
    // the main thread when that line moved. Same cure. The tip is one
    // stored long; reading it through Wallet.balances decoded every output
    // the wallet has ever owned to keep a single field.
    var pending by remember { mutableStateOf<List<Ledger.OpenRequest>>(emptyList()) }
    var tip by remember { mutableStateOf(0L) }
    var tabs by remember { mutableStateOf<List<org.ducatproject.ducat.RunningTab>>(emptyList()) }
    var settled by remember { mutableStateOf(false) }
    LaunchedEffect(version) {
        withContext(Dispatchers.IO) {
            events = Ledger.build(context)
            pending = Ledger.openRequests(context)
            tip = org.ducatproject.ducat.WalletStore(context).tip()
            tabs = org.ducatproject.ducat.TabStore(context).all()
        }
        settled = true
    }
    var open by remember { mutableStateOf<Ledger.Event?>(null) }

    // Ask the chain about anything still unclassified as soon as this screen is
    // looked at, rather than waiting up to a poll interval. Until a transaction
    // has been read, change from an outgoing payment is indistinguishable from
    // an incoming one, and this is the screen where that distinction is the
    // entire content.
    //
    // Keyed on the rows, not the store version: the rows arrive from the
    // read above some time after the version moved, and an effect keyed on
    // the version ran before they did, found the previous list, and left
    // the first look at this screen to wait for the poll after all. Each
    // enrichment that resolves something bumps the store, which rebuilds
    // the rows, which runs this again over whatever is still open; one
    // that resolves nothing bumps nothing, so there is no chasing.
    LaunchedEffect(events) {
        // Only what a fetch could actually resolve. Keying on "chain == null"
        // would also match rows that have no transaction id to look up, so the
        // condition would never go false and the screen would re-enrich on
        // every unrelated change to the store.
        if (events.any { it.provisional }) {
            withContext(Dispatchers.IO) {
                NodeStore(context).lastGood()?.let { Ledger.enrich(context, it) }
            }
        }
    }

    open?.let { e ->
        TxDetailScreen(e, tip) { open = null }
        return
    }

    // **Find it again.** A thread is where a bill was agreed, but this is where
    // it was *paid*, and until now the only way back to a receipt from six
    // weeks ago was to scroll until it appeared. What people actually remember
    // is what they bought — so the lines inside a receipt are searched, not
    // only the name on the outside of it.
    var query by rememberSaveable { mutableStateOf("") }
    val q = query.trim().lowercase()

    // Built once per change to the ledger, not once per keystroke. Formatting
    // an amount means a rate lookup and a locale pass, and doing that for
    // every row on every letter typed is how a search box comes to feel
    // broken on the exact wallet that most needs one.
    val haystacks = remember(events) {
        events.map { e ->
            Triple(
                e,
                buildString {
                    e.counterparty?.let { append(it).append(' ') }
                    e.note?.let { append(it).append(' ') }
                    // The receipt's own lines. This is the point of the box.
                    e.items.forEach { append(it.description).append(' ') }
                    // What the row *says*, so typing the figure someone is
                    // looking at finds the row they are looking at — rather
                    // than the piconero underneath it, which nobody has seen.
                    append(Amounts.show(context, e.amountPxmr).primary)
                }.lowercase(),
                // Identifiers, kept apart because they match differently.
                buildString {
                    e.address?.let { append(it).append(' ') }
                    e.escrow?.let { append(it).append(' ') }
                    append(e.txid)
                }.lowercase(),
            )
        }
    }
    // **Words search words; only a long query searches identifiers.**
    //
    // A Monero address is ninety-odd characters of base58 and a txid is
    // sixty-four of hex, so a short word lands inside one of them by chance —
    // "flat" pulled up an unrelated payment whose address happened to contain
    // those four letters next to the receipt it was meant to find. Six
    // characters is past where that happens by accident and well short of
    // what somebody pasting part of a txid would type.
    val ids = q.length >= 6
    // Tax time in one tap: only the payments that went into donate-card
    // threads, with what they add up to. The chip only exists once there is
    // a donation to show — a filter for money nobody has given is clutter.
    var donationsOnly by rememberSaveable { mutableStateOf(false) }
    // The glanceable answer above the statement: a period, four figures,
    // and — when there was trade — the business's own line. The tiles are
    // filters, not decoration: In shows only money in, Out only money out,
    // so the summary and its evidence stay one screen.
    var period by rememberSaveable { mutableStateOf(2) } // 0 today, 1 7d, 2 month, 3 all
    var dirOnly by rememberSaveable { mutableStateOf(0) } // 0 both, 1 in, 2 out
    val zone = java.time.ZoneId.systemDefault()
    val fromTs = when (period) {
        0 -> java.time.LocalDate.now(zone).atStartOfDay(zone).toEpochSecond()
        1 -> System.currentTimeMillis() / 1000 - 7 * 86_400
        2 -> java.time.LocalDate.now(zone).withDayOfMonth(1).atStartOfDay(zone).toEpochSecond()
        else -> 0L
    }
    val summary = remember(events, fromTs) {
        Ledger.summarize(events, fromTs, Long.MAX_VALUE)
    }
    // Three shops on one phone must not share an undifferentiated take:
    // when the roster has more than one persona, the business line can be
    // narrowed to one. Wallet tiles stay whole-wallet — there is one purse.
    val personas = remember { org.ducatproject.ducat.PersonaStore(context).all() }
    var bizPersona by rememberSaveable { mutableStateOf<String?>(null) }
    val bizAll = remember(tabs, fromTs) {
        Ledger.summarizeBusiness(tabs, fromTs, Long.MAX_VALUE)
    }
    val biz = remember(tabs, fromTs, bizPersona) {
        if (bizPersona == null) bizAll
        else Ledger.summarizeBusiness(
            tabs.filter { it.personaHex == bizPersona }, fromTs, Long.MAX_VALUE,
        )
    }
    val searched = if (q.isEmpty()) events else haystacks
        .filter { (_, text, id) -> text.contains(q) || (ids && id.contains(q)) }
        .map { it.first }
    val ranged = searched.filter { it.pending || it.timestamp >= fromTs }
        .filter {
            when (dirOnly) {
                1 -> it.direction == Ledger.Direction.Received
                2 -> it.direction == Ledger.Direction.Sent
                else -> true
            }
        }
    val shownEvents = if (donationsOnly) ranged.filter { it.donation } else ranged
    val shownPending = if (donationsOnly) emptyList() else
        if (q.isEmpty()) pending else pending.filter {
            it.counterparty.lowercase().contains(q) ||
                Amounts.show(context, it.amountPxmr).primary.lowercase().contains(q)
        }

    // Nothing settled *and* nothing awaiting: a bill that arrived before the
    // wallet's first transaction is exactly the uncleared half this
    // statement shows, and "Nothing yet" over it said the wallet was new
    // while somebody was waiting to be paid.
    if (events.isEmpty() && pending.isEmpty()) {
        // "Nothing yet" waits for the ledger to have actually been built
        // once: a history-sized read runs off-main, and the empty copy
        // painted during it told a wallet with months of history it was
        // new. A quiet spinner is the honest interim.
        if (!settled) {
            Column(
                Modifier.fillMaxSize().padding(32.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
            ) {
                CircularProgressIndicator()
            }
            return
        }
        Column(
            Modifier.fillMaxSize().padding(32.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Icon(
                Icons.Filled.Receipt, null, Modifier.size(48.dp),
                tint = MaterialTheme.colorScheme.outline,
            )
            Spacer(Modifier.height(12.dp))
            Text(stringResource(R.string.activity_nothing_yet), style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(6.dp))
            Text(
                stringResource(R.string.activity_empty_desc),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        return
    }

    Column(Modifier.fillMaxSize()) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        OutlinedTextField(
            value = query,
            onValueChange = { query = it },
            singleLine = true,
            placeholder = { Text(stringResource(R.string.activity_search_hint)) },
            leadingIcon = { Icon(Icons.Filled.Search, null) },
            trailingIcon = {
                if (query.isNotEmpty()) {
                    IconButton(onClick = { query = "" }) {
                        Icon(Icons.Filled.Close, stringResource(R.string.activity_search_clear))
                    }
                }
            },
            modifier = Modifier.weight(1f),
        )
        // The statement, out. Everything this screen shows, as CSV through
        // the share sheet — see Ledger.exportCsv for what a row carries and
        // why there is deliberately no fiat column.
        val scope = androidx.compose.runtime.rememberCoroutineScope()
        var exportMenu by remember { mutableStateOf(false) }
        fun share(json: Boolean) {
            scope.launch {
                withContext(Dispatchers.IO) {
                    runCatching {
                        val body = if (json) Ledger.exportJson(context) else Ledger.exportCsv(context)
                        val dir = java.io.File(context.filesDir, "backups").apply { mkdirs() }
                        val f = java.io.File(
                            dir, if (json) "ducat-statement.json" else "ducat-statement.csv",
                        )
                        f.writeText(body)
                        f
                    }
                }.onSuccess { f ->
                    runCatching {
                        val uri = androidx.core.content.FileProvider.getUriForFile(
                            context, "${context.packageName}.backups", f,
                        )
                        val send = android.content.Intent(android.content.Intent.ACTION_SEND).apply {
                            type = if (json) "application/json" else "text/csv"
                            putExtra(android.content.Intent.EXTRA_STREAM, uri)
                            addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION)
                        }
                        context.startActivity(
                            android.content.Intent.createChooser(
                                send, context.getString(R.string.activity_export_share),
                            ),
                        )
                    }
                }
            }
        }
        Box {
            IconButton(onClick = { exportMenu = true }) {
                Icon(
                    Icons.Filled.IosShare,
                    stringResource(R.string.activity_export),
                )
            }
            DropdownMenu(expanded = exportMenu, onDismissRequest = { exportMenu = false }) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.activity_export_csv)) },
                    onClick = { exportMenu = false; share(json = false) },
                )
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.activity_export_json)) },
                    onClick = { exportMenu = false; share(json = true) },
                )
            }
        }
    }
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        listOf(
            R.string.activity_period_today, R.string.activity_period_7d,
            R.string.activity_period_month, R.string.activity_period_all,
        ).forEachIndexed { i, res ->
            FilterChip(
                selected = period == i,
                onClick = { period = i },
                label = { Text(stringResource(res), style = MaterialTheme.typography.labelSmall) },
            )
        }
    }
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        @Composable
        fun tile(label: Int, pxmr: Long, filter: Int, tint: androidx.compose.ui.graphics.Color?) {
            val active = dirOnly == filter && filter != 0
            Surface(
                onClick = {
                    if (filter != 0) dirOnly = if (dirOnly == filter) 0 else filter
                },
                enabled = filter != 0,
                shape = MaterialTheme.shapes.medium,
                color = if (active) {
                    MaterialTheme.colorScheme.secondaryContainer
                } else {
                    MaterialTheme.colorScheme.surfaceVariant
                },
                modifier = Modifier.weight(1f),
            ) {
                Column(Modifier.padding(horizontal = 8.dp, vertical = 6.dp)) {
                    Text(
                        stringResource(label),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Text(
                        Amounts.show(context, pxmr).primary,
                        style = MaterialTheme.typography.labelMedium,
                        maxLines = 1,
                        color = tint ?: MaterialTheme.colorScheme.onSurface,
                    )
                }
            }
        }
        tile(R.string.activity_sum_in, summary.inPxmr, 1, MaterialTheme.ducat.settled)
        tile(R.string.activity_sum_out, summary.outPxmr, 2, null)
        tile(R.string.activity_sum_net, summary.netPxmr, 0, null)
        tile(R.string.activity_sum_fees, summary.feesPxmr, 0, null)
    }
    if (bizAll.salesCount > 0 || bizAll.outstandingCount > 0 || bizAll.taxCollectedPxmr > 0) {
        Surface(
            shape = MaterialTheme.shapes.medium,
            color = MaterialTheme.colorScheme.surfaceVariant,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        ) {
            Column(Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        stringResource(R.string.activity_business),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    if (personas.size > 1) {
                        Spacer(Modifier.weight(1f))
                        Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                            FilterChip(
                                selected = bizPersona == null,
                                onClick = { bizPersona = null },
                                label = {
                                    Text(
                                        stringResource(R.string.activity_period_all),
                                        style = MaterialTheme.typography.labelSmall,
                                    )
                                },
                            )
                            personas.forEach { pers ->
                                FilterChip(
                                    selected = bizPersona == pers.hex,
                                    onClick = { bizPersona = pers.hex },
                                    label = {
                                        Text(
                                            pers.name.ifBlank {
                                                stringResource(R.string.personas_primary)
                                            },
                                            style = MaterialTheme.typography.labelSmall,
                                        )
                                    },
                                )
                            }
                        }
                    }
                }
                Spacer(Modifier.height(2.dp))
                val doorNames = mapOf(
                    "pos" to stringResource(R.string.activity_door_pos),
                    "bar" to stringResource(R.string.activity_door_bar),
                    "taxi" to stringResource(R.string.activity_door_taxi),
                    "donate" to stringResource(R.string.activity_door_donate),
                    "pub" to stringResource(R.string.activity_door_pub),
                )
                val doors = biz.byOrigin.entries.joinToString(" · ") { (origin, d) ->
                    "${doorNames[origin] ?: origin} ${d.count} · " +
                        Amounts.show(context, d.takePxmr).primary
                }
                if (biz.salesCount > 0) {
                    Text(
                        "${stringResource(R.string.activity_sales)} ${biz.salesCount} · " +
                            Amounts.show(context, biz.salesPxmr).primary +
                            if (doors.isNotEmpty()) "  ($doors)" else "",
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                if (biz.taxCollectedPxmr > 0) {
                    Text(
                        "${stringResource(R.string.activity_tax_collected)}: " +
                            Amounts.show(context, biz.taxCollectedPxmr).primary,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                if (biz.outstandingCount > 0) {
                    Text(
                        "${stringResource(R.string.activity_outstanding)}: " +
                            "${biz.outstandingCount} · " +
                            Amounts.show(context, biz.outstandingPxmr).primary,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.tertiary,
                    )
                }
            }
        }
        Spacer(Modifier.height(4.dp))
    }
    if (events.any { it.donation }) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            FilterChip(
                selected = donationsOnly,
                onClick = { donationsOnly = !donationsOnly },
                label = { Text(stringResource(R.string.activity_donations_filter)) },
            )
            if (donationsOnly) {
                Spacer(Modifier.weight(1f))
                // What the filtered rows add up to — the figure the tax form
                // wants, beside the rows that justify it. Amounts, not fees:
                // the fee bought carriage, not the cause.
                Text(
                    stringResource(
                        R.string.activity_donated_total,
                        Amounts.show(
                            context, shownEvents.sumOf { it.amountPxmr },
                        ).primary,
                    ),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.ducat.settled,
                )
            }
        }
        Spacer(Modifier.height(4.dp))
    }
    if (shownEvents.isEmpty() && shownPending.isEmpty()) {
        // The ledger has rows (the "Nothing yet" screen would have taken
        // an empty one), so an empty list here is a narrowing: the search,
        // the period, or a tile. Say which — four zero tiles over blank
        // space on the first of the month, "This month" selected by
        // default, read as a wallet that had lost its history.
        Text(
            when {
                q.isNotEmpty() -> stringResource(R.string.activity_search_none, query)
                period != 3 -> stringResource(R.string.activity_period_none)
                else -> stringResource(R.string.activity_filter_none)
            },
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 24.dp),
        )
        return@Column
    }
    LazyColumn(Modifier.fillMaxSize()) {
        // The uncleared half of the statement: money asked for and not yet
        // moved, in either direction. A bank shows pending card holds for the
        // same reason — the balance alone under-describes the situation.
        if (shownPending.isNotEmpty()) {
            item {
                Text(
                    stringResource(R.string.activity_awaiting),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(start = 16.dp, top = 12.dp, bottom = 4.dp),
                )
            }
            items(shownPending) { r ->
                val shown = Amounts.show(context, r.amountPxmr)
                ListItem(
                    modifier = Modifier.clickable {
                        org.ducatproject.ducat.MainActivity.openChat.value = r.contactHex
                    },
                    colors = ListItemDefaults.colors(
                        containerColor = androidx.compose.ui.graphics.Color.Transparent,
                    ),
                    leadingContent = {
                        Icon(
                            Icons.Filled.Schedule, null,
                            tint = MaterialTheme.colorScheme.outline,
                        )
                    },
                    headlineContent = {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            // Weighted and clipped, or a 32-character name
                            // measures first, the spacer collapses, and the
                            // amount — the row's whole point — renders at
                            // zero width.
                            Text(
                                if (r.theyAsked)
                                    stringResource(R.string.activity_they_ask, r.counterparty)
                                else stringResource(R.string.activity_you_asked, r.counterparty),
                                style = MaterialTheme.typography.bodyMedium,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f),
                            )
                            Spacer(Modifier.width(8.dp))
                            Text(shown.primary, fontWeight = FontWeight.SemiBold)
                        }
                    },
                    supportingContent = {
                        Text(
                            (if (r.items.isNotEmpty())
                                pluralStringResource(
                                    R.plurals.activity_items, r.items.size, r.items.size,
                                ) + " · " else "") + whenText(context, r.timestamp),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.outline,
                        )
                    },
                )
            }
            item {
                HorizontalDivider(
                    color = MaterialTheme.colorScheme.outlineVariant,
                    modifier = Modifier.padding(vertical = 4.dp),
                )
            }
        }
        items(shownEvents) { e -> EventRow(e) { open = e } }
    }
    }
}

@Composable
private fun EventRow(e: Ledger.Event, onClick: () -> Unit) {
    val context = LocalContext.current
    val sent = e.direction == Ledger.Direction.Sent
    val amount = Amounts.show(context, e.amountPxmr)
    val balance = Amounts.show(context, e.balanceAfterPxmr)

    ListItem(
        modifier = Modifier.clickable(onClick = onClick),
        // On the page itself. ListItem's default fill is a tonal band, and a
        // list of bands reads as a table; rows on the background with space
        // between them read as a feed, which is the register every payments
        // app the user knows writes in. The ruling lines went for the same
        // reason — the avatars already separate the rows.
        colors = ListItemDefaults.colors(containerColor = androidx.compose.ui.graphics.Color.Transparent),
        leadingContent = {
            Icon(
                when {
                    e.pending -> Icons.Filled.Schedule
                    e.unexplained -> Icons.AutoMirrored.Filled.HelpOutline
                    sent -> Icons.Filled.ArrowUpward
                    e.locked -> Icons.Filled.Lock
                    else -> Icons.Filled.ArrowDownward
                },
                null,
                tint = when {
                    e.pending -> MaterialTheme.colorScheme.outline
                    e.unexplained -> MaterialTheme.colorScheme.error
                    // Out and in are different directions *and* different
                    // colours. Both arrows pointed the same way before, which
                    // made a send indistinguishable from a receipt at a glance.
                    sent -> MaterialTheme.ducat.changePending
                    e.locked -> MaterialTheme.ducat.changePending
                    else -> MaterialTheme.ducat.settled
                },
            )
        },
        headlineContent = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    "${if (sent) "−" else "+"}${amount.primary}",
                    fontWeight = FontWeight.SemiBold,
                )
                Spacer(Modifier.weight(1f))
                // The balance after this row, so the column reconciles with
                // Accounts. Dimmer than the amount: it is context, not the event.
                if (!e.pending) {
                    Text(
                        balance.primary,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        },
        supportingContent = {
            Column {
                Row {
                    amount.secondary?.let {
                        Text(
                            it,
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.outline,
                        )
                    }
                    Spacer(Modifier.weight(1f))
                    if (!e.pending) {
                        balance.secondary?.let {
                            Text(
                                it,
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.outline,
                            )
                        }
                    }
                }
                Spacer(Modifier.height(2.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        if (e.provisional) stringResource(R.string.activity_reading_transaction)
                        else who(context, e),
                        style = MaterialTheme.typography.bodySmall,
                        color = if (e.provisional) MaterialTheme.ducat.changePending
                        else MaterialTheme.colorScheme.onSurface,
                    )
                    if (e.receipted) {
                        Spacer(Modifier.width(6.dp))
                        // The paperwork chip: this payment has a receipt in a
                        // thread, which is what separates a DUCAT payment from
                        // a bare transfer.
                        Text(
                            stringResource(R.string.activity_receipt_chip),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.ducat.settled,
                        )
                    }
                    if (e.donation) {
                        Spacer(Modifier.width(6.dp))
                        Text(
                            stringResource(R.string.activity_donation_chip),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.primary,
                        )
                    }
                }
                e.note?.let {
                    Text(
                        stringResource(R.string.activity_note_quoted, it),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                    )
                }
                Text(
                    when {
                        e.pending -> stringResource(R.string.activity_sending)
                        e.unexplained -> stringResource(R.string.activity_spent_unknown)
                        // Until the transaction is read, this could be change
                        // from a payment you made rather than money arriving.
                        e.provisional -> stringResource(R.string.activity_maybe_change)
                        e.locked -> stringResource(
                            R.string.activity_locked_when,
                            whenText(context, e.timestamp),
                            androidx.compose.ui.res.pluralStringResource(
                                R.plurals.activity_unlocks_blocks,
                                e.unlocksInBlocks.toInt(), e.unlocksInBlocks.toInt(),
                            ),
                        )
                        else -> whenText(context, e.timestamp)
                    },
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.outline,
                )
            }
        },
    )
}

/**
 * Who was on the other side, and never a guess.
 *
 * A received payment usually has no answer: Monero does not carry a sender, and
 * the only thing that can supply one is a contact's own notice naming the
 * transaction (§16.13). Saying "unknown sender" is the honest result, and
 * inventing anything else would be inventing a counterparty.
 */
/**
 * What one movement of money should be called.
 *
 * Shared with the Home screen's Recent list, which used to answer this
 * question itself and answer it worse: `counterparty ?: "Sent"/"Received"`,
 * so an escrow deposit read as a bare "Sent" — the very thing the escrow
 * branch below exists to prevent — and a payment to a pasted address lost the
 * address. Both surfaces name a movement the same way now, which is also one
 * fewer place for the next rule to be added to only once.
 */
internal fun who(context: Context, e: Ledger.Event): String {
    val sent = e.direction == Ledger.Direction.Sent
    // An escrow first, because it explains the movement in a way no address
    // and no absent sender can: money going into one has not been spent, and
    // money coming out of one is your own deposit or your own earnings
    // arriving — not a stranger paying you, which is what "sender unknown"
    // reads as to the person holding the phone.
    e.escrow?.let { title ->
        return when {
            title.isNotBlank() && sent ->
                context.getString(R.string.activity_escrow_in, title)
            title.isNotBlank() ->
                context.getString(R.string.activity_escrow_out, title)
            sent -> context.getString(R.string.activity_escrow_in_untitled)
            else -> context.getString(R.string.activity_escrow_out_untitled)
        }
    }
    e.counterparty?.let {
        return if (sent) context.getString(R.string.activity_to, it)
        else context.getString(R.string.activity_from, it)
    }
    e.address?.let {
        return context.getString(R.string.activity_to, "${it.take(12)}…${it.takeLast(6)}")
    }
    return if (sent) context.getString(R.string.activity_sent_unrecorded)
    else context.getString(R.string.activity_received_unknown)
}

/** A date and a time, because a block height is not a moment to anybody. */
internal fun whenText(context: Context, epochSecs: Long): String {
    if (epochSecs <= 0) return context.getString(R.string.txdetail_time_unknown)
    val d = java.util.Date(epochSecs * 1000)
    // Date from the locale — getDateInstance, not a "d MMM yyyy" pattern,
    // because the field order is the locale's to decide: Japanese and
    // Chinese write year first, and a pinned pattern gave everyone the
    // English order. Time from the phone's own 12/24-hour setting.
    return java.text.DateFormat.getDateInstance(java.text.DateFormat.MEDIUM).format(d) +
        ", " + android.text.format.DateFormat.getTimeFormat(context).format(d)
}
