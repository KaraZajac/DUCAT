package org.ducatproject.desk

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.Publications
import org.ducatproject.ducat.Swarm
import org.ducatproject.ducat.TabStore
import org.ducatproject.ducat.formatXmr

/**
 * The publisher's room: where a month leaves the building.
 *
 * The desk is the publishing machine on purpose — serving a swarm share
 * costs cores the phone will not pay until load-shedding lands, and a
 * publication's master key deserves the machine with the backups anyway.
 *
 * The operator decides WHO (the roster — §16.20 puts no subscription
 * object on the wire) and WHEN (a settled payment, a comp, a trial;
 * automation of settle→send stays on the post-1.0 list). This room only
 * makes the mechanics honest: seed, send, and a ledger of what shipped
 * where — because serving dies with the process, and a relaunch must not
 * mean re-deriving your own history from chat threads.
 *
 * One live seed at a time (the engine's stop-slot holds one token); the
 * Seed-again button per issue is the after-relaunch path.
 */
@Composable
fun PublishRoom() {
    val context = androidx.compose.ui.platform.LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val scope = rememberCoroutineScope()

    val pubs = remember(version) { Publications.publications(context) }
    var selected by remember { mutableStateOf(pubs.firstOrNull()?.first) }
    if (selected != null && pubs.none { it.first == selected }) selected = null

    Row(Modifier.fillMaxSize()) {
        // The shelf of publications, and the door to a new one.
        Column(Modifier.width(230.dp).fillMaxHeight().padding(12.dp)) {
            Text("Publications", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(8.dp))
            pubs.forEach { (id, title) ->
                NavigationDrawerItem(
                    label = { Text(title.ifBlank { id.take(8) }) },
                    selected = id == selected,
                    onClick = { selected = id },
                )
            }
            Spacer(Modifier.height(16.dp))
            var newTitle by remember { mutableStateOf("") }
            OutlinedTextField(
                newTitle, { if (it.length <= 40) newTitle = it },
                label = { Text("New publication") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(6.dp))
            Button(
                onClick = {
                    selected = Publications.create(context, newTitle.trim())
                    newTitle = ""
                },
                enabled = newTitle.isNotBlank(),
            ) { Text("Create") }
        }
        VerticalDivider()

        val pubId = selected
        // An else, not an early return: a bare return out of a Compose
        // lambda is the IntStack.peek2 crash that hides on exactly this
        // kind of empty-state branch.
        if (pubId == null) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text(
                    "A publication is a master key and a name.\n" +
                        "Issues are sealed per period; subscribers get each period's key.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else Column(Modifier.weight(1f).fillMaxHeight().verticalScroll(rememberScrollState()).padding(16.dp)) {
            // --- the roster -----------------------------------------------
            Text("Subscribers", style = MaterialTheme.typography.titleMedium)
            Text(
                "Who receives each issue's key. Yours to decide — after a settled " +
                    "payment, as a comp, however this publication sells.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            val roster = remember(version, pubId) {
                Publications.subscribers(context, pubId).toSet()
            }
            val contacts = remember(version) { ContactStore(context).all() }
            if (contacts.isEmpty()) {
                Text(
                    "No contacts yet — a subscriber claims your card first.",
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            contacts.forEach { c ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(
                        checked = c.personaHex in roster,
                        onCheckedChange = {
                            Publications.setSubscriber(context, pubId, c.personaHex, it)
                        },
                    )
                    Text(c.displayName())
                }
            }

            Spacer(Modifier.height(20.dp))
            HorizontalDivider()
            Spacer(Modifier.height(20.dp))

            // --- the period, priced and billed ----------------------------
            Text("This period", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(8.dp))
            val now = remember { java.time.YearMonth.now().toString() }
            var period by remember(pubId) { mutableStateOf(now) }
            var busy by remember { mutableStateOf<String?>(null) }
            var lastWord by remember { mutableStateOf<String?>(null) }
            OutlinedTextField(
                period, { if (it.length <= 64) period = it },
                label = { Text("Period (e.g. $now)") }, singleLine = true,
            )
            Spacer(Modifier.height(6.dp))
            // A price makes the room run §15.11's reconcile: bill the
            // roster, and whoever the chain shows as paid gets the issue on
            // the poll clock — no price means a free publication, sent to
            // the whole roster on seed.
            val storedPrice = remember(version, pubId) { Publications.priceOf(context, pubId) }
            var priceText by remember(pubId) {
                mutableStateOf(if (storedPrice > 0) formatXmr(storedPrice) else "")
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                OutlinedTextField(
                    priceText, { priceText = it },
                    label = { Text("Price per period (XMR, blank = free)") },
                    singleLine = true,
                )
                Spacer(Modifier.width(8.dp))
                TextButton(onClick = {
                    val pxmr = if (priceText.isBlank()) 0L
                    else Amounts.parse(priceText)?.let { Amounts.toPxmr(it) } ?: -1L
                    if (pxmr >= 0) Publications.setPrice(context, pubId, pxmr)
                }) { Text("Set") }
            }
            if (storedPrice > 0) {
                val billed = remember(version, pubId, period, busy) {
                    Publications.billedFor(context, pubId, period.trim())
                }
                val paidCount = remember(version, billed) {
                    val tabs = TabStore(context)
                    billed.values.count { tabs.get(it)?.state?.startsWith("paid") == true }
                }
                Text(
                    "Billed ${billed.size} · paid $paidCount. Paid subscribers get " +
                        "the issue automatically once it is seeded.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(6.dp))
                Button(
                    enabled = busy == null && roster.isNotEmpty() && period.isNotBlank(),
                    onClick = {
                        busy = "Billing…"
                        scope.launch(Dispatchers.IO) {
                            try {
                                val n = Publications.billPeriod(context, pubId, period.trim())
                                lastWord = "Billed $n subscriber(s) for ${period.trim()} at " +
                                    "${formatXmr(storedPrice)} XMR."
                            } finally {
                                busy = null
                            }
                        }
                    },
                ) { Text("Bill roster") }
                Spacer(Modifier.height(16.dp))
            }

            // --- shipping the issue ---------------------------------------
            Text("New issue", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(8.dp))
            var path by remember(pubId) { mutableStateOf("") }
            var note by remember(pubId) { mutableStateOf("") }
            OutlinedTextField(
                path, { path = it },
                label = { Text("File to ship (path)") }, singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(6.dp))
            OutlinedTextField(
                note, { if (it.length <= 500) note = it },
                label = { Text("Note to subscribers (optional)") }, singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(8.dp))
            Button(
                enabled = busy == null && period.isNotBlank() &&
                    File(path.trim()).isFile && roster.isNotEmpty(),
                onClick = {
                    val f = path.trim()
                    val p = period.trim()
                    busy = "Seeding…"
                    lastWord = null
                    scope.launch(Dispatchers.IO) {
                        try {
                            val share = Swarm.seed(f)
                            Publications.recordIssue(
                                context, pubId, p, f,
                                share.shareKey, share.indexDigestHex,
                            )
                            if (storedPrice > 0) {
                                // Paid publication: the reconcile owns
                                // delivery. Run it now for anyone already
                                // settled; the poll clock catches the rest.
                                busy = "Sending to the paid…"
                                Publications.reconcileSettled(context)
                                val got = Publications.issues(context, pubId)
                                    .firstOrNull { it.periodId == p }?.sentTo?.size ?: 0
                                lastWord = "Issue $p seeded; sent to $got already-paid " +
                                    "subscriber(s). Others get it when their payment lands."
                            } else {
                                busy = "Sending…"
                                var sent = 0
                                val readers = ContactStore(context).all()
                                    .filter { it.personaHex in Publications.subscribers(context, pubId) }
                                for (c in readers) {
                                    val ok = Publications.sendPeriod(
                                        context, c, pubId, p,
                                        record = null, headKey = null,
                                        note = note.trim(),
                                        swarmKey = share.shareKey,
                                        swarmDigestHex = share.indexDigestHex,
                                    )
                                    if (ok) {
                                        Publications.markSent(context, pubId, p, c.personaHex)
                                        sent++
                                    }
                                }
                                lastWord = "Issue $p: seeded and sent to $sent of ${readers.size}. " +
                                    "Serving from this desk while it runs."
                            }
                        } catch (e: Throwable) {
                            DucatLog.w("Publish", "issue $p failed: ${e.message}")
                            lastWord = "Failed: ${e.message}"
                        } finally {
                            busy = null
                        }
                    }
                },
            ) { Text(busy ?: if (storedPrice > 0) "Seed issue" else "Seed + send") }
            lastWord?.let {
                Spacer(Modifier.height(6.dp))
                Text(it, style = MaterialTheme.typography.bodySmall)
            }

            Spacer(Modifier.height(20.dp))
            HorizontalDivider()
            Spacer(Modifier.height(20.dp))

            // --- the log --------------------------------------------------
            Text("Shipped issues", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(8.dp))
            val issues = remember(version, pubId, busy) { Publications.issues(context, pubId) }
            if (issues.isEmpty()) {
                Text(
                    "Nothing shipped yet.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            issues.forEach { issue ->
                Row(Modifier.fillMaxWidth().padding(vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                    val issueBilled = remember(version, pubId, issue.periodId) {
                        Publications.billedFor(context, pubId, issue.periodId)
                    }
                    val issuePaid = remember(version, issueBilled) {
                        val tabs = TabStore(context)
                        issueBilled.values.count { tabs.get(it)?.state?.startsWith("paid") == true }
                    }
                    Column(Modifier.weight(1f)) {
                        Text("Issue ${issue.periodId}", style = MaterialTheme.typography.bodyLarge)
                        Text(
                            (if (issueBilled.isNotEmpty()) "billed ${issueBilled.size} · paid $issuePaid · " else "") +
                                "sent ${issue.sentTo.size} · ${File(issue.file).name}",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    val missing = roster - issue.sentTo
                    // A paid publication never offers a blanket send — the
                    // reconcile delivers to the settled; this button would
                    // hand the issue to whoever has not paid.
                    if (missing.isNotEmpty() && storedPrice == 0L) {
                        TextButton(enabled = busy == null, onClick = {
                            busy = "Sending…"
                            scope.launch(Dispatchers.IO) {
                                try {
                                    val readers = ContactStore(context).all()
                                        .filter { it.personaHex in missing }
                                    for (c in readers) {
                                        val ok = Publications.sendPeriod(
                                            context, c, pubId, issue.periodId,
                                            record = null, headKey = null, note = "",
                                            swarmKey = issue.swarmKey,
                                            swarmDigestHex = issue.swarmDigestHex,
                                        )
                                        if (ok) Publications.markSent(
                                            context, pubId, issue.periodId, c.personaHex,
                                        )
                                    }
                                } finally {
                                    busy = null
                                }
                            }
                        }) { Text("Send to ${missing.size} more") }
                    }
                    TextButton(
                        enabled = busy == null && File(issue.file).isFile,
                        onClick = {
                            busy = "Seeding…"
                            scope.launch(Dispatchers.IO) {
                                try {
                                    // A fresh share for the same bytes: relaunches
                                    // mint new routes, so the shipment is re-sent
                                    // to everyone who should hold the new pair.
                                    val share = Swarm.seed(issue.file)
                                    Publications.recordIssue(
                                        context, pubId, issue.periodId, issue.file,
                                        share.shareKey, share.indexDigestHex,
                                    )
                                    val readers = ContactStore(context).all()
                                        .filter { it.personaHex in issue.sentTo }
                                    for (c in readers) {
                                        Publications.sendPeriod(
                                            context, c, pubId, issue.periodId,
                                            record = null, headKey = null, note = "",
                                            swarmKey = share.shareKey,
                                            swarmDigestHex = share.indexDigestHex,
                                        )
                                    }
                                } catch (e: Throwable) {
                                    DucatLog.w("Publish", "re-seed failed: ${e.message}")
                                } finally {
                                    busy = null
                                }
                            }
                        },
                    ) { Text("Seed again") }
                }
            }
        }
    }
}
