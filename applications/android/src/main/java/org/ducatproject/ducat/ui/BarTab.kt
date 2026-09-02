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
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
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
import org.ducatproject.ducat.R
import org.ducatproject.ducat.RunningTab
import org.ducatproject.ducat.TabStore
import org.ducatproject.ducat.formatXmr

private const val TAG = "BarTab"

/** States with nothing left to reconcile — safe to drop from the tab book. */
private val CLOSED_STATES = setOf("paid", "paid_oob", "cancelled")

/**
 * Whether one of this tab's own messages has left the phone.
 *
 * A send commits its row before the network (Mailbox.sendLocked), so a row
 * can sit undelivered for as long as the phone is out of range — `Later`,
 * and the poll delivers it — or for ever, when the log it was numbered in
 * has been retired by a re-claim since: the frozen read mark is what says
 * so (StoredMessage.readByThem), and that is `Never`.
 */
private enum class Sent { Yes, Later, Never }

private fun sentState(m: org.ducatproject.ducat.StoredMessage): Sent = when {
    m.delivered -> Sent.Yes
    m.readByThem == null -> Sent.Later
    else -> Sent.Never
}

/** What the thread says about a tab: see [TabDetail]. */
private data class TabSaid(
    val refused: Boolean = false,
    val bill: Sent = Sent.Yes,
    val word: Sent = Sent.Yes,
)

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
fun BarTabScreen(
    /** "all" (legacy), "open" (running + billed + start), or "closed". The
     *  bar shell splits the night's two halves across two tabs. */
    section: String = "all",
) {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val store = remember { TabStore(context) }
    // Every origin, not just the bar's own: a taxi fare or a till sale that
    // was billed and then abandoned needs the same two exits (cancel, settled
    // outside), and this list is the only place that manages settlements.
    val tabs = remember(version) { store.all() }
    // One store pass per change, off the main thread, instead of a
    // ContactStore.all() and a thread decode inside every row — .all() on
    // EncryptedSharedPreferences decrypts every entry per call, and a busy
    // night's tab book was paying that per row per frame.
    var contactsByHex by remember { mutableStateOf<Map<String, org.ducatproject.ducat.Contact>>(emptyMap()) }
    var refusedIds by remember { mutableStateOf<Set<String>>(emptySet()) }
    LaunchedEffect(version) {
        val (byHex, refused) = withContext(Dispatchers.IO) {
            val contacts = ContactStore(context)
            val byHex = contacts.all().associateBy { it.personaHex }
            // **Declined is not the same news as unpaid.** A customer who
            // refuses a bill sends a kind-5 naming it; this list — the one a
            // counter actually works from — must not read that as "billed,
            // unpaid", indistinguishable from somebody who has not got round
            // to it. The bill is the one settle sent for the tab
            // (RunningTab.billIn); the amount match only covers tabs billed
            // before the seq was kept.
            val refused = tabs.filter { it.state == "settled" }.mapNotNull { t ->
                val thread = contacts.thread(t.personaHex)
                t.billIn(thread)
                    ?.takeIf { (it.seq to it.timestamp) in billAnswers(thread).refused }
                    ?.let { t.id }
            }.toSet()
            byHex to refused
        }
        contactsByHex = byHex; refusedIds = refused
    }
    // Saveable, like the till's sale: a bartender who turned the phone while
    // a customer was scanning the code was back on the list, with the code
    // gone and a 12-hour card out that nothing was watching any more.
    var openId by rememberSaveable { mutableStateOf<String?>(null) }
    var opening by rememberSaveable { mutableStateOf(false) }

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
    // Everything finished, cancelled included — a cancelled tab that appears
    // nowhere is not gone, it is just unmanageable.
    val done = tabs.filter { it.state in CLOSED_STATES }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        if (section != "closed") {
        // Sixteen, like the button under it and like every other screen —
        // see Pos.kt. At twenty-four the "Start a tab" button overhung the
        // words above and below it by eight pixels.
        Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
            Spacer(Modifier.height(16.dp))
            Text(
                stringResource(R.string.bartab_open_tabs),
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
        ) { Text(stringResource(R.string.bartab_start_tab)) }

        }
        if (section != "closed" && open.isNotEmpty()) {
            SectionLabel(stringResource(R.string.bartab_section_running))
            Card(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
                Column {
                    open.forEach { t -> TabRow(t, contactsByHex[t.personaHex], t.id in refusedIds) { openId = t.id } }
                }
            }
        }

        if (section != "closed" && awaiting.isNotEmpty()) {
            SectionLabel(stringResource(R.string.bartab_section_billed))
            Card(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
                Column {
                    awaiting.forEach { t -> TabRow(t, contactsByHex[t.personaHex], t.id in refusedIds) { openId = t.id } }
                }
            }
            Text(
                stringResource(R.string.bartab_receipt_auto_hint),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.outline,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp),
            )
        }

        if (section != "open" && done.isNotEmpty()) {
            Row(
                Modifier.fillMaxWidth().padding(end = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                SectionLabel(stringResource(R.string.bartab_section_settled))
                Spacer(Modifier.weight(1f))
                // Local bookkeeping only: the receipts already live in the
                // threads, which is where the record of the night belongs.
                // The bar's own tabs; a kiosk order or a subscription reads
                // its tab after it closes (RunningTab.keptElsewhere), and
                // clearing those put a paid coffee back to "awaiting" on the
                // kiosk and lost a paid subscriber their issue.
                val clearable = done.filterNot { it.keptElsewhere }
                if (clearable.isNotEmpty()) {
                    TextButton(onClick = { clearable.forEach { store.delete(it.id) } }) {
                        Text(stringResource(R.string.bartab_clear), style = MaterialTheme.typography.labelMedium)
                    }
                }
            }
            Card(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
                Column { done.forEach { t -> TabRow(t, contactsByHex[t.personaHex], t.id in refusedIds) { openId = t.id } } }
            }
        }

        if (section == "closed" && done.isEmpty()) {
            Text(
                stringResource(R.string.bartab_nothing_settled),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(16.dp),
            )
        }
        // Nothing running *on this page* — not nothing in the book. Once the
        // first tab of the night had closed, the Tabs page showed a total of
        // zero over a blank space with no word on it.
        if (section != "closed" && open.isEmpty() && awaiting.isEmpty() &&
            (section == "open" || done.isEmpty())
        ) {
            Text(
                stringResource(R.string.bartab_no_tabs),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp),
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
private fun TabRow(
    t: RunningTab,
    contact: org.ducatproject.ducat.Contact?,
    refused: Boolean,
    onClick: () -> Unit,
) {
    val context = LocalContext.current
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
            val status = when (t.state) {
                "open" -> pluralStringResource(R.plurals.bartab_items, t.lines.size, t.lines.size)
                "settled" -> when {
                    t.seenTx != null -> stringResource(R.string.bartab_state_payment_seen)
                    refused -> stringResource(R.string.bartab_state_declined)
                    else -> stringResource(R.string.bartab_state_billed_unpaid)
                }
                "paid_oob" -> stringResource(R.string.bartab_state_paid_oob)
                "cancelled" -> stringResource(R.string.bartab_state_cancelled)
                // A tip is named here for the same reason it is on the till's
                // own sales list: it is the line a counter cares most about,
                // and this row was showing the bill as though nothing had been
                // left on top of it.
                else -> if (t.tipPxmr > 0) stringResource(
                    R.string.shells_paid_incl_tip,
                    Amounts.show(context, t.tipPxmr).primary,
                ) else stringResource(R.string.bartab_state_paid)
            }
            Text(
                buildString {
                    if (t.origin != "bar") append("${t.origin} · ")
                    append(status)
                },
                style = MaterialTheme.typography.labelSmall,
                color = when (t.state) {
                    "paid", "paid_oob" -> MaterialTheme.ducat.settled
                    else -> MaterialTheme.colorScheme.onSurfaceVariant
                },
            )
        }
        // The till's own two-line amount, rather than a second copy of it that
        // had drifted: this one still led with XMR and then printed
        // `secondary` beneath, which is XMR again once the local currency
        // leads, so a tab's total read the same piconero figure twice.
        //
        // `takePxmr`, not `totalPxmr`: what a closed tab brought in, which on
        // a tipped one is more than it billed.
        AmountBoth(t.takePxmr)
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
    // The card outlives the screen instance: issued once per opening, not
    // once per rotation, so the code the customer is already scanning stays
    // the code this screen waits on.
    var cardUri by rememberSaveable { mutableStateOf<String?>(null) }
    var cardInbox by rememberSaveable { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }

    BackHandler(onBack = onBack)

    DisposableEffect(cardUri) {
        org.ducatproject.ducat.nfc.Tap.offered = cardUri
        onDispose { org.ducatproject.ducat.nfc.Tap.offered = null }
    }

    LaunchedEffect(Unit) {
        if (cardUri != null) return@LaunchedEffect
        val r = withContext(Dispatchers.IO) {
            runCatching {
                Mailbox.issueCard(
                    context, MyProfile(context).name(), 60uL * 60uL * 12uL, purpose = "sale",
                )
            }
        }
        r.onSuccess { cardUri = it.uri; cardInbox = it.inboxKey }
            .onFailure { error = moneyFailure(context, it) }
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
            // Resumed if they already have one running: a regular who
            // scanned the code instead of being tapped on the list — or was
            // tapped, then scanned — must not end the night with two tabs
            // and two bills.
            onOpened(store.openOrResume(fresh.personaHex, "bar"))
            break
        }
    }

    // Keyed: a tab is started against a contact, and the contact you just
    // added is exactly the one you are about to start it for.
    val contactsV by ContactStore.changes.collectAsState()
    var regulars by remember { mutableStateOf<List<org.ducatproject.ducat.Contact>>(emptyList()) }
    // Which of these names belong to more than one person. Two regulars called
    // Sam are two identical rows, and picking the wrong one bills somebody who
    // is not standing at the bar — the till says "delivered", the customer in
    // front of you never sees it, and nothing about either screen says why.
    // Pay's picker has shown the key on ambiguous rows since it was written;
    // this one is the same question asked at the same moment.
    var ambiguous by remember { mutableStateOf<Set<String>>(emptySet()) }
    LaunchedEffect(contactsV) {
        val (r, a) = withContext(Dispatchers.IO) {
            val contacts = ContactStore(context)
            contacts.all().filter { it.theirBundle != null }
                .sortedBy { it.displayName().lowercase() } to contacts.ambiguous()
        }
        regulars = r; ambiguous = a
    }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(stringResource(R.string.bartab_new_customer), style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(10.dp))
        when {
            cardUri != null -> {
                QrBlock(cardUri!!)
                Spacer(Modifier.height(8.dp))
                Text(
                    stringResource(R.string.bartab_scan_hint),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            error != null -> Text(error!!, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall)
            else -> CatSpinner(Modifier.size(40.dp), tint = MaterialTheme.colorScheme.primary)
        }

        if (regulars.isNotEmpty()) {
            Spacer(Modifier.height(24.dp))
            Text(stringResource(R.string.bartab_or_regular), style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(6.dp))
            Card(Modifier.fillMaxWidth()) {
                Column {
                    regulars.forEach { c ->
                        Row(
                            Modifier.fillMaxWidth()
                                .clickable { onOpened(store.openOrResume(c.personaHex, "bar")) }
                                .padding(horizontal = 16.dp, vertical = 10.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Avatar(c.displayName(), c.avatar)
                            Spacer(Modifier.width(12.dp))
                            Column {
                                Text(c.displayName(), style = MaterialTheme.typography.bodyLarge)
                                if (c.personaHex in ambiguous) {
                                    Text(
                                        stringResource(
                                            R.string.pay_name_shared_key,
                                            c.personaHex.take(16),
                                        ),
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }

        Spacer(Modifier.height(20.dp))
        OutlinedButton(onClick = onBack, modifier = Modifier.fillMaxWidth().height(48.dp)) {
            Text(stringResource(R.string.bartab_back))
        }
    }
}

/**
 * Withdraw a bill with the wire's word for it, when the bill can be named.
 *
 * A kind-5 Retract pointing at the original request lets the customer's
 * client grey out its own Review button instead of trusting them to read a
 * sentence. The request's seq is recovered from the thread — the last
 * outgoing bill for the same total — and when it cannot be, the plain-text
 * cancellation goes out exactly as before (§15.11: telling them imperfectly
 * still beats not telling them). A Retract carries no amount; core refuses
 * one that does.
 *
 * Through [TabStore.close], like the plain cancellation: a retract that
 * never left the phone puts the tab back to billed and throws, because the
 * failure used to be logged and walked away from — the screen said
 * "cancelled" over a bill the customer could still pay. Null when the tab
 * was not there to cancel, or not billed and unpaid any more.
 */
internal fun cancelTabWithRetract(
    context: android.content.Context,
    store: TabStore,
    tab: RunningTab,
): RunningTab? {
    val contacts = ContactStore(context)
    // The stored seq when the tab has one; the amount match only for tabs
    // billed before settle recorded it.
    val billSeq = tab.billSeq.takeIf { it >= 0 }
        ?: contacts.thread(tab.personaHex)
            .lastOrNull { it.outgoing && it.kind == 1 && it.amountPxmr == tab.settledTotal }
            ?.seq
        ?: return store.cancel(tab)
    return store.close(tab, "cancelled", revert = true) { contact ->
        Mailbox.send(
            context, contact,
            context.getString(
                R.string.bartab_bill_cancelled_msg,
                Amounts.show(context, tab.settledTotal).primary,
            ),
            kind = 5, reSeq = billSeq, reOwn = true,
        )
    }?.also {
        DucatLog.i(TAG, "${tab.origin} tab cancelled (${formatXmr(tab.settledTotal)} XMR)")
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
    var confirmDiscard by remember { mutableStateOf(false) }
    // Off the main thread: .all() decrypts the whole contact book per call.
    val contact by produceState<org.ducatproject.ducat.Contact?>(null, tab.personaHex) {
        value = withContext(Dispatchers.IO) {
            ContactStore(context).all().firstOrNull { it.personaHex == tab.personaHex }
        }
    }
    val name = contact?.displayName() ?: "${tab.personaHex.take(8)}…"
    // What the thread says about this tab's own messages — whether the bill
    // was refused, and whether the bill or the closing word has actually
    // left the phone. Read off the main thread, and again on every store
    // change: the poll delivers a late slot and the mark clears itself.
    val v by ContactStore.changes.collectAsState()
    val said by produceState(TabSaid(), tab.id, tab.state, tab.wordSeq, v) {
        value = withContext(Dispatchers.IO) {
            val thread = ContactStore(context).thread(tab.personaHex)
            val bill = tab.billIn(thread)
            TabSaid(
                refused = bill?.let { (it.seq to it.timestamp) in billAnswers(thread).refused } ?: false,
                bill = bill?.let { sentState(it) } ?: Sent.Yes,
                word = when {
                    tab.wordSeq == RunningTab.WORD_UNSENT -> Sent.Never
                    tab.wordSeq < 0 -> Sent.Yes
                    // The newest row at that seq, the rule markDelivered
                    // ticks by: a re-claim restarts the numbering and the
                    // thread keeps the old log's rows.
                    else -> thread.lastOrNull { it.outgoing && it.seq == tab.wordSeq }
                        ?.let { sentState(it) } ?: Sent.Yes
                },
            )
        }
    }

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
            // One path for both ways of adding a drink — tapped off the
            // menu or typed by hand. A tab that told the customer about one
            // and not the other would be sending them half the evening.
            fun addLine(d: String, a: Long) {
                // Onto the tab as it stands, not as this screen last drew it.
                // Two chips tapped inside one frame both read the same `tab`,
                // and the second write dropped the first drink; and a tab that
                // was billed a moment ago must not quietly take another line
                // after the customer has the total in their hand.
                var added = false
                val updated = store.mutate(tab.id) {
                    if (it.state != "open") it
                    else { added = true; it.copy(lines = it.lines + BillItem(d, a)) }
                } ?: return
                if (!added) return
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
                            context.getString(
                                R.string.bartab_drink_notice,
                                d,
                                Amounts.show(context, a).primary,
                                Amounts.show(context, updated.totalPxmr).primary,
                            ),
                        )
                    }.onFailure { DucatLog.w(TAG, "drink notice: ${it.message}") }
                }
            }
            ItemPicker { name, pxmr -> addLine(name, pxmr) }
            PosAddLine { d, a -> addLine(d, a) }
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
                                maxLines = 2, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                                style = MaterialTheme.typography.bodyLarge)
                            Text(Amounts.show(context, line.amountPxmr).primary,
                                style = MaterialTheme.typography.bodyMedium,
                                fontFamily = FontFamily.Monospace)
                            if (tab.state == "open") {
                                IconButton(
                                    onClick = {
                                        // By index against the current lines,
                                        // which is safe because the only other
                                        // writer appends: a drink poured while
                                        // this row was being tapped lands after
                                        // it and leaves i naming the same one.
                                        store.mutate(tab.id) { t ->
                                            if (t.state != "open") t
                                            else t.copy(
                                                lines = t.lines.filterIndexed { j, _ -> j != i },
                                            )
                                        }
                                    },
                                    // 40dp, not the 30 it wore: an explicit
                                    // size overrides the 48dp the component
                                    // reserves, and this tap deletes a line
                                    // of a live tab — the one button in the
                                    // row that must not be the hardest to
                                    // hit.
                                    modifier = Modifier.size(40.dp),
                                ) { Icon(Icons.Filled.Close, stringResource(R.string.bartab_remove_line), Modifier.size(16.dp)) }
                            }
                        }
                    }
                }
            }
        }

        Spacer(Modifier.height(16.dp))
        when (tab.state) {
            "open" -> {
                // A standing rate is applied at the moment of billing — the
                // one moment the subtotal is final — and the button says the
                // number it is about to bill, tax in, because a label reading
                // 6.50 above a bill for 7.04 is the till lying by omission.
                val standingTax =
                    if (org.ducatproject.ducat.Tax.enabled(context) && (tab.taxPxmr ?: 0L) <= 0L)
                        org.ducatproject.ducat.Tax.on(context, tab.lines.sumOf { it.amountPxmr })
                    else 0L
                Button(
                    onClick = {
                        busy = true; error = null
                        scope.launch {
                            val r = withContext(Dispatchers.IO) {
                                runCatching {
                                    val toSettle = if (standingTax > 0)
                                        store.mutate(tab.id) { it.copy(taxPxmr = standingTax) }!!
                                    else tab
                                    store.settle(toSettle)
                                }
                            }
                            busy = false
                            r.onFailure { error = moneyFailure(context, it) }
                        }
                    },
                    enabled = !busy && tab.lines.isNotEmpty(),
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp).height(52.dp),
                ) {
                    if (busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                    else Text(stringResource(R.string.bartab_settle_bill, Amounts.show(context, tab.totalPxmr + standingTax).primary))
                }
                Text(
                    // Fenced: a name that runs the other way took the rest
                    // of the sentence with it.
                    stringResource(R.string.bartab_settle_hint, isolate(name)),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.outline,
                    modifier = Modifier.padding(horizontal = 24.dp, vertical = 6.dp),
                )
                TextButton(
                    onClick = { confirmDiscard = true },
                    modifier = Modifier.padding(horizontal = 16.dp),
                ) { Text(stringResource(R.string.bartab_discard_tab), color = MaterialTheme.colorScheme.error) }

                // An open tab discarded takes its line items with it and cannot
                // be recovered, so it asks first — the same courtesy Forget gets.
                if (confirmDiscard) {
                    AlertDialog(
                        onDismissRequest = { confirmDiscard = false },
                        title = { Text(stringResource(R.string.bartab_discard_title)) },
                        text = {
                            Text(stringResource(R.string.bartab_discard_body))
                        },
                        confirmButton = {
                            TextButton(onClick = {
                                confirmDiscard = false; store.delete(tab.id); onBack()
                            }) { Text(stringResource(R.string.bartab_discard_confirm), color = MaterialTheme.colorScheme.error) }
                        },
                        dismissButton = {
                            TextButton(onClick = { confirmDiscard = false }) { Text(stringResource(R.string.bartab_keep_tab)) }
                        },
                    )
                }
            }
            // Their payment is in the network. The till's own words for it:
            // the receipt sends itself when the chain agrees, and the two
            // exits below are gone — cancelling now would strand a payment
            // that is already on its way (TabStore.close refuses it too),
            // and "paid outside" would receipt the wrong rail.
            "settled" -> if (tab.seenTx != null) Column(Modifier.padding(horizontal = 24.dp)) {
                Text(
                    stringResource(R.string.pos_payment_seen),
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.ducat.changePending,
                )
                Text(
                    stringResource(R.string.pos_payment_settling),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else Column(Modifier.padding(horizontal = 16.dp)) {
                // The same news the list carries, on the screen somebody opens
                // to decide what to do about it. Promising a receipt "when the
                // payment lands" to a counter whose customer has refused the
                // bill is the wrong sentence twice over — and to one whose
                // bill is still on this phone, the wrong one once more.
                Text(
                    stringResource(
                        when {
                            said.refused -> R.string.bartab_declined_hint
                            said.bill == Sent.Yes -> R.string.bartab_billed_hint
                            said.bill == Sent.Later -> R.string.bartab_bill_pending
                            else -> R.string.bartab_bill_unsent
                        },
                    ),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 8.dp),
                )
                Spacer(Modifier.height(12.dp))
                // Cash across the bar is a settlement, not an exception — the
                // fallback rails existing is half the design. They still get a
                // receipt in the thread; it just points at no transaction.
                Button(
                    enabled = !busy,
                    onClick = {
                        // Awaited, and only then away. Leaving first meant the
                        // bar was back at the tab list believing the tab was
                        // closed while the receipt had not gone anywhere —
                        // and there was no second chance to notice, because
                        // the screen that would have said so was gone.
                        busy = true; error = null
                        scope.launch {
                            withContext(Dispatchers.IO) {
                                runCatching { store.markPaidOutside(tab) }
                            }
                                .onSuccess { onBack() }
                                .onFailure { error = moneyFailure(context, it) }
                            busy = false
                        }
                    },
                    modifier = Modifier.fillMaxWidth().height(48.dp),
                ) { Text(stringResource(R.string.bartab_paid_outside_button)) }
                Spacer(Modifier.height(8.dp))
                // Withdrawing the bill tells them in the thread — their app
                // still shows a Review button pointing at money nobody is
                // watching for, and a cancellation they never hear about is a
                // payment into the void.
                OutlinedButton(
                    enabled = !busy,
                    onClick = {
                        // The note above this button says a cancellation they
                        // never hear about is a payment into the void, and the
                        // code then dropped the failure and walked away. Now
                        // it stays until the retract has gone.
                        busy = true; error = null
                        scope.launch {
                            withContext(Dispatchers.IO) {
                                runCatching { cancelTabWithRetract(context, store, tab) }
                            }
                                .onSuccess { onBack() }
                                .onFailure { error = moneyFailure(context, it) }
                            busy = false
                        }
                    },
                    modifier = Modifier.fillMaxWidth().height(48.dp),
                ) { Text(stringResource(R.string.bartab_cancel_bill), color = MaterialTheme.colorScheme.error) }
            }
            // "They were told" only once the notice has left the phone. Until
            // the poll delivers it, the tab is closed and they are not — and
            // a notice numbered in a log a re-claim has since retired will
            // never go: nothing for it but to say so in the conversation.
            "cancelled" -> Text(
                stringResource(
                    when (said.word) {
                        Sent.Yes -> R.string.bartab_cancelled_note
                        Sent.Later -> R.string.bartab_cancelled_pending
                        Sent.Never -> R.string.bartab_cancelled_unsent
                    },
                ),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
            "paid_oob" -> Column(Modifier.padding(horizontal = 24.dp)) {
                Text(
                    stringResource(
                        when (said.word) {
                            Sent.Yes -> R.string.bartab_paid_oob_note
                            Sent.Later -> R.string.bartab_paid_oob_pending
                            Sent.Never -> R.string.bartab_paid_oob_unsent
                        },
                    ),
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.ducat.settled,
                )
                // The receipt that did not go, offered again from the tab
                // that owes it — the money is already in the till, and the
                // customer's record should not depend on the bar's wifi at
                // the moment the cash changed hands.
                if (said.word == Sent.Never) {
                    TextButton(
                        enabled = !busy,
                        onClick = {
                            busy = true; error = null
                            scope.launch {
                                withContext(Dispatchers.IO) {
                                    runCatching { store.sendOobReceipt(tab) }
                                }.onFailure { error = moneyFailure(context, it) }
                                busy = false
                            }
                        },
                    ) { Text(stringResource(R.string.bartab_send_receipt)) }
                }
            }
            else -> Text(
                stringResource(R.string.bartab_paid_note),
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.ducat.settled,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
        }

        if (tab.state in CLOSED_STATES && !tab.keptElsewhere) {
            // Only closed tabs: an open tab has "Discard", a billed one must
            // exit through cancel or paid-outside so the customer is told —
            // deleting it here would leave their app pointing at a bill nobody
            // is watching for. And only the bar's own: a kiosk order or a
            // subscription still reads its tab after it closes
            // (RunningTab.keptElsewhere).
            TextButton(
                onClick = { store.delete(tab.id); onBack() },
                modifier = Modifier.padding(horizontal = 16.dp),
            ) { Text(stringResource(R.string.bartab_remove_from_book), color = MaterialTheme.colorScheme.error) }
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
        ) { Text(stringResource(R.string.bartab_back_to_tabs)) }
        Spacer(Modifier.height(24.dp))
    }
}
