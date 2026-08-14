package org.ducatproject.ducat.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.QrCode2
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
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.RunningTab
import org.ducatproject.ducat.TabStore
import org.ducatproject.ducat.formatXmr

private const val TAG = "BarTab"

/**
 * The tab book (§15.11): one running account per customer, settled as one bill.
 *
 * The evening's shape: open a tab when someone orders their first drink — a
 * scan for a stranger, a tap on a name for a regular — add lines as the night
 * goes on, and settle when they close out. The network sees nothing until
 * settlement, then one itemised request in the tab's own conversation; the
 * receipt follows the payment automatically, even if they pay from the bus
 * home. That last part is the §16.12 machinery earning its keep: the thread
 * outlives the visit.
 */
@Composable
fun BarTabScreen() {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val store = remember { TabStore(context) }
    // Every origin, not just the bar's own: a taxi fare or a till sale that
    // was billed and then abandoned needs the same two exits (cancel, settled
    // outside), and this list is the only place that manages settlements.
    val tabs = remember(version) { store.all() }
    var openId by remember { mutableStateOf<String?>(null) }
    var opening by remember { mutableStateOf(false) }

    openId?.let { id ->
        val tab = tabs.firstOrNull { it.id == id }
        if (tab != null) {
            TabDetail(tab, onBack = { openId = null })
            return
        }
        openId = null
    }

    if (opening) {
        OpenTab(
            onOpened = { opening = false; openId = it.id },
            onBack = { opening = false },
        )
        return
    }

    val open = tabs.filter { it.state == "open" && it.origin == "bar" }
    val awaiting = tabs.filter { it.state == "settled" }
    val paid = tabs.filter { it.state == "paid" || it.state == "paid_oob" }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp)) {
            Spacer(Modifier.height(16.dp))
            Text(
                "Open tabs",
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            val outstanding = open.sumOf { it.totalPxmr }
            val shown = Amounts.show(context, outstanding)
            Text(shown.primary, style = MaterialTheme.typography.displayLarge)
            shown.secondary?.let {
                Text(it, style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Spacer(Modifier.height(16.dp))
        }

        Button(
            onClick = { opening = true },
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp).height(52.dp),
        ) { Text("Start a tab") }

        if (open.isNotEmpty()) {
            SectionLabel("Running")
            Card(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
                Column {
                    open.forEach { t -> TabRow(t) { openId = t.id } }
                }
            }
        }

        if (awaiting.isNotEmpty()) {
            SectionLabel("Billed — waiting for payment")
            Card(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
                Column {
                    awaiting.forEach { t -> TabRow(t) { openId = t.id } }
                }
            }
            Text(
                "The receipt goes to them by itself the moment the payment lands " +
                    "— even after they have left.",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.outline,
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 6.dp),
            )
        }

        if (paid.isNotEmpty()) {
            SectionLabel("Settled tonight")
            Card(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
                Column { paid.forEach { t -> TabRow(t) { openId = t.id } } }
            }
        }

        if (tabs.isEmpty()) {
            Text(
                "No tabs yet. Start one when the first order lands.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
        }
        Spacer(Modifier.height(24.dp))
    }
}

@Composable
private fun SectionLabel(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.labelMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(start = 24.dp, top = 20.dp, bottom = 8.dp),
    )
}

@Composable
private fun TabRow(t: RunningTab, onClick: () -> Unit) {
    val context = LocalContext.current
    val contact = remember(t.personaHex) {
        ContactStore(context).all().firstOrNull { it.personaHex == t.personaHex }
    }
    val name = contact?.displayName() ?: "${t.personaHex.take(8)}…"
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Avatar(name, contact?.avatar)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(name, style = MaterialTheme.typography.titleMedium)
            Text(
                buildString {
                    if (t.origin != "bar") append("${t.origin} · ")
                    append(
                        when (t.state) {
                            "open" -> "${t.lines.size} item(s)"
                            "settled" -> "billed, unpaid"
                            "paid_oob" -> "paid outside DUCAT ✓"
                            "cancelled" -> "cancelled"
                            else -> "paid ✓"
                        }
                    )
                },
                style = MaterialTheme.typography.labelSmall,
                color = when (t.state) {
                    "paid", "paid_oob" -> MaterialTheme.ducat.settled
                    else -> MaterialTheme.colorScheme.onSurfaceVariant
                },
            )
        }
        Column(horizontalAlignment = Alignment.End) {
            Text(
                "${formatXmr(t.totalPxmr)} XMR",
                style = MaterialTheme.typography.bodyMedium,
                fontFamily = FontFamily.Monospace,
            )
            Amounts.show(context, t.totalPxmr).secondary?.let {
                Text(it, style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.outline)
            }
        }
    }
}

/**
 * A stranger scans; a regular is a tap on their name.
 *
 * The same card machinery as everywhere else — the tab's conversation *is* the
 * relationship, so a regular already has one and gets no second code.
 */
@Composable
private fun OpenTab(onOpened: (RunningTab) -> Unit, onBack: () -> Unit) {
    val context = LocalContext.current
    val store = remember { TabStore(context) }
    var cardUri by remember { mutableStateOf<String?>(null) }
    var cardInbox by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }

    BackHandler(onBack = onBack)

    DisposableEffect(cardUri) {
        org.ducatproject.ducat.nfc.Tap.offered = cardUri
        onDispose { org.ducatproject.ducat.nfc.Tap.offered = null }
    }

    LaunchedEffect(Unit) {
        val r = withContext(Dispatchers.IO) {
            runCatching {
                Mailbox.issueCard(
                    context, MyProfile(context).name(), 60uL * 60uL * 12uL, purpose = "sale",
                )
            }
        }
        r.onSuccess { cardUri = it.uri; cardInbox = it.inboxKey }
            .onFailure { error = it.message ?: "could not publish a code" }
    }

    // A scan opens the tab by itself — the bartender should not need a second
    // tap while holding a shaker. Bound to *this* card's claimant: someone
    // scanning the profile code at the same moment must not land on a tab.
    LaunchedEffect(cardInbox) {
        val inbox = cardInbox ?: return@LaunchedEffect
        while (true) {
            delay(2_000)
            val fresh = withContext(Dispatchers.IO) {
                runCatching {
                    Mailbox.collectClaims(context)
                    ContactStore(context).claimantOf(inbox)?.let { hex ->
                        ContactStore(context).all().firstOrNull { it.personaHex == hex }
                    }
                }.getOrNull()
            } ?: continue
            DucatLog.i(TAG, "tab opened by ${fresh.displayName()}")
            onOpened(store.open(fresh.personaHex, "bar"))
            break
        }
    }

    val regulars = remember {
        ContactStore(context).all().filter { it.theirBundle != null }
    }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("New customer", style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(10.dp))
        when {
            cardUri != null -> {
                QrBlock(cardUri!!)
                Spacer(Modifier.height(8.dp))
                Text(
                    "They scan or tap — the tab opens by itself.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            error != null -> Text(error!!, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall)
            else -> CircularProgressIndicator()
        }

        if (regulars.isNotEmpty()) {
            Spacer(Modifier.height(24.dp))
            Text("Or a regular", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(6.dp))
            Card(Modifier.fillMaxWidth()) {
                Column {
                    regulars.forEach { c ->
                        Row(
                            Modifier.fillMaxWidth()
                                .clickable { onOpened(store.open(c.personaHex, "bar")) }
                                .padding(horizontal = 16.dp, vertical = 10.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Avatar(c.displayName(), c.avatar)
                            Spacer(Modifier.width(12.dp))
                            Text(c.displayName(), style = MaterialTheme.typography.bodyLarge)
                        }
                    }
                }
            }
        }

        Spacer(Modifier.height(20.dp))
        OutlinedButton(onClick = onBack, modifier = Modifier.fillMaxWidth().height(48.dp)) {
            Text("Back")
        }
    }
}

/** One tab: its lines, and the one button that bills it. */
@Composable
private fun TabDetail(tab: RunningTab, onBack: () -> Unit) {
    val context = LocalContext.current
    val store = remember { TabStore(context) }
    val scope = rememberCoroutineScope()
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val contact = remember(tab.personaHex) {
        ContactStore(context).all().firstOrNull { it.personaHex == tab.personaHex }
    }
    val name = contact?.displayName() ?: "${tab.personaHex.take(8)}…"

    BackHandler(onBack = onBack)

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Avatar(name, contact?.avatar, size = 48)
            Spacer(Modifier.width(12.dp))
            Column {
                Text(name, style = MaterialTheme.typography.headlineSmall)
                val shown = Amounts.show(context, tab.totalPxmr)
                Text(
                    shown.primary + (shown.secondary?.let { " · $it" } ?: ""),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        if (tab.state == "open") {
            PosAddLine { d, a ->
                val updated = tab.copy(lines = tab.lines + BillItem(d, a))
                store.update(updated)
                // Tell them, so the tab is never a surprise at close. A text
                // message, deliberately — §15.11 forbids per-line payment
                // requests, because five confirm screens for one evening is
                // five where one was owed. Best-effort: a notice that fails
                // must not block the next order.
                scope.launch(Dispatchers.IO) {
                    runCatching {
                        val c = ContactStore(context).all()
                            .first { it.personaHex == tab.personaHex }
                        Mailbox.send(
                            context, c,
                            "$d — ${formatXmr(a)} XMR · tab now " +
                                "${formatXmr(updated.totalPxmr)} XMR",
                            org.ducatproject.ducat.PersonaStore(context).personaHex(),
                        )
                    }.onFailure { DucatLog.w(TAG, "drink notice: ${it.message}") }
                }
            }
        }

        if (tab.lines.isNotEmpty()) {
            Spacer(Modifier.height(10.dp))
            Card(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
                Column(Modifier.padding(horizontal = 16.dp, vertical = 6.dp)) {
                    tab.lines.forEachIndexed { i, line ->
                        Row(
                            Modifier.fillMaxWidth().padding(vertical = 4.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(line.description, Modifier.weight(1f),
                                style = MaterialTheme.typography.bodyLarge)
                            Text("${formatXmr(line.amountPxmr)} XMR",
                                style = MaterialTheme.typography.bodyMedium,
                                fontFamily = FontFamily.Monospace)
                            if (tab.state == "open") {
                                IconButton(
                                    onClick = {
                                        store.update(tab.copy(
                                            lines = tab.lines.filterIndexed { j, _ -> j != i }
                                        ))
                                    },
                                    modifier = Modifier.size(30.dp),
                                ) { Icon(Icons.Filled.Close, "Remove", Modifier.size(14.dp)) }
                            }
                        }
                    }
                }
            }
        }

        Spacer(Modifier.height(16.dp))
        when (tab.state) {
            "open" -> {
                Button(
                    onClick = {
                        busy = true; error = null
                        scope.launch {
                            val r = withContext(Dispatchers.IO) {
                                runCatching { store.settle(tab) }
                            }
                            busy = false
                            r.onFailure { error = it.message ?: "could not send the bill" }
                        }
                    },
                    enabled = !busy && tab.lines.isNotEmpty(),
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp).height(52.dp),
                ) {
                    if (busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                    else Text("Settle — bill ${Amounts.show(context, tab.totalPxmr).primary}")
                }
                Text(
                    "One itemised bill into your conversation with $name. They can " +
                        "pay from anywhere — the thread does not close when they leave.",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.outline,
                    modifier = Modifier.padding(horizontal = 24.dp, vertical = 6.dp),
                )
                TextButton(
                    onClick = { store.delete(tab.id); onBack() },
                    modifier = Modifier.padding(horizontal = 16.dp),
                ) { Text("Discard tab", color = MaterialTheme.colorScheme.error) }
            }
            "settled" -> Column(Modifier.padding(horizontal = 16.dp)) {
                Text(
                    "Billed. The receipt goes to them by itself when the payment " +
                        "lands. If it was settled some other way, say so here:",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 8.dp),
                )
                Spacer(Modifier.height(12.dp))
                // Cash across the bar is a settlement, not an exception — the
                // fallback rails existing is half the design. They still get a
                // receipt in the thread; it just points at no transaction.
                Button(
                    onClick = {
                        scope.launch(Dispatchers.IO) { runCatching { store.markPaidOutside(tab) } }
                        onBack()
                    },
                    modifier = Modifier.fillMaxWidth().height(48.dp),
                ) { Text("Paid outside DUCAT (cash, card)") }
                Spacer(Modifier.height(8.dp))
                // Withdrawing the bill tells them in the thread — their app
                // still shows a Review button pointing at money nobody is
                // watching for, and a cancellation they never hear about is a
                // payment into the void.
                OutlinedButton(
                    onClick = {
                        scope.launch(Dispatchers.IO) { runCatching { store.cancel(tab) } }
                        onBack()
                    },
                    modifier = Modifier.fillMaxWidth().height(48.dp),
                ) { Text("Cancel the bill", color = MaterialTheme.colorScheme.error) }
            }
            "cancelled" -> Text(
                "Cancelled — they were told in the conversation.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
            "paid_oob" -> Text(
                "Paid outside DUCAT ✓ — receipt sent, no transaction attached.",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.ducat.settled,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
            else -> Text(
                "Paid ✓ — receipt sent.",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.ducat.settled,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
        }

        error?.let {
            Text(it, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 6.dp))
        }

        Spacer(Modifier.height(12.dp))
        OutlinedButton(
            onClick = onBack,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp).height(48.dp),
        ) { Text("Back to tabs") }
        Spacer(Modifier.height(24.dp))
    }
}
