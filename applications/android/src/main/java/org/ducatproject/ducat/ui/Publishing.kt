package org.ducatproject.ducat.ui

import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.MenuBook
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import java.io.File
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.Publications
import org.ducatproject.ducat.R
import org.ducatproject.ducat.Swarm
import org.ducatproject.ducat.TabStore
import org.ducatproject.ducat.formatXmr
import org.ducatproject.ducat.saidWhy

/**
 * Publishing, from the device in your pocket (§16.20) — unlocked the day
 * the load-shedding hunt showed seeding costs a phone ~7 points over
 * being a phone at all.
 *
 * The same rails as the desk's Publish room, phone-shaped: one scrolling
 * screen, the rail picked by size (a month the shelf holds goes onto DHT
 * records and survives this phone going dark; a heavier one seeds the
 * swarm while the app runs), the roster scoped to the worn persona like
 * every list, and settlement — not attention — opening the mailbag when
 * a price is set.
 */
@Composable
fun PublishingSection() {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()

    val pubs = remember(version) { Publications.publications(context) }
    // The form below is saveable field by field, like every other form in
    // the app: the activity is recreated on rotation, and a note typed and
    // a file picked for this month's issue were gone when the phone turned.
    var selected by rememberSaveable { mutableStateOf(pubs.firstOrNull()?.first) }
    if (selected == null || pubs.none { it.first == selected }) selected = pubs.firstOrNull()?.first
    var busy by remember { mutableStateOf<String?>(null) }
    var lastWord by remember { mutableStateOf<String?>(null) }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        // Saveable, with the title inside it: a rotation closed the dialog
        // and took the name somebody was typing with it.
        var creating by rememberSaveable { mutableStateOf(false) }

        // --- which publication, and the door to a new one -----------------
        //
        // Creation is summoned, never ambient: a desk you manage things at
        // does not keep a blank form on the counter. With no publications
        // the whole screen is the invitation; with some, a small + at the
        // end of the row.
        if (pubs.isEmpty()) {
            Column(
                Modifier.fillMaxWidth().padding(top = 72.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Box(
                    Modifier.size(72.dp)
                        .clip(CircleShape)
                        .background(MaterialTheme.colorScheme.surfaceVariant),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        Icons.AutoMirrored.Filled.MenuBook,
                        contentDescription = null,
                        modifier = Modifier.size(36.dp),
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Spacer(Modifier.height(16.dp))
                Text(
                    stringResource(R.string.pub_empty_title),
                    style = MaterialTheme.typography.titleLarge,
                )
                Spacer(Modifier.height(8.dp))
                Text(
                    stringResource(R.string.pub_intro),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
                Spacer(Modifier.height(20.dp))
                Button(onClick = { creating = true }) {
                    Text(stringResource(R.string.pub_create_btn))
                }
                Spacer(Modifier.height(16.dp))
                Text(
                    stringResource(R.string.pub_none_yet),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
            }
        } else {
            Text(
                stringResource(R.string.pub_intro),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                pubs.forEach { (id, title) ->
                    FilterChip(
                        selected = id == selected,
                        onClick = { selected = id },
                        label = { Text(title.ifBlank { id.take(8) }) },
                    )
                }
                IconButton(onClick = { creating = true }) {
                    Icon(
                        Icons.Filled.Add,
                        contentDescription = stringResource(R.string.pub_create_btn),
                    )
                }
            }
        }
        if (creating) {
            var newTitle by rememberSaveable { mutableStateOf("") }
            AlertDialog(
                onDismissRequest = { creating = false },
                title = { Text(stringResource(R.string.pub_new_name)) },
                text = {
                    OutlinedTextField(
                        newTitle, { if (it.length <= 40) newTitle = it },
                        label = { Text(stringResource(R.string.pub_name_label)) },
                        singleLine = true,
                    )
                },
                confirmButton = {
                    TextButton(
                        enabled = newTitle.isNotBlank(),
                        onClick = {
                            selected = Publications.create(context, newTitle.trim())
                            creating = false
                        },
                    ) { Text(stringResource(R.string.pub_create)) }
                },
                dismissButton = {
                    TextButton(onClick = { creating = false }) {
                        Text(stringResource(R.string.common_cancel))
                    }
                },
            )
        }

        // Scan-to-subscribe: a publish-purpose card, QR'd. Each opening
        // mints a fresh week-long card bound to this publication; every
        // claim of any still-valid one enrolls the claimant.
        var subCode by remember { mutableStateOf<String?>(null) }
        subCode?.let { uri ->
            androidx.compose.material3.AlertDialog(
                onDismissRequest = { subCode = null },
                confirmButton = {
                    TextButton(onClick = { subCode = null }) {
                        Text(stringResource(R.string.pub_code_done))
                    }
                },
                title = { Text(stringResource(R.string.pub_sub_code)) },
                text = {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        QrBlock(uri)
                        Spacer(Modifier.height(8.dp))
                        Text(
                            stringResource(R.string.pub_sub_code_hint),
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                },
            )
        }

        val pubId = selected
        if (pubId != null) {
            // Every job on this screen runs off it (ThreadSends), one key
            // each, and what it needs back — the busy flags, the word, the
            // code, the staged path — is read here when the registry
            // ticks. They ran on the screen's own scope, where the bodies
            // finished (nothing in them suspends) but their last lines
            // landed on a screen that was gone: an issue published through
            // a rotation left `stagedPath` set on the recreated form, with
            // the file and the note still in it and Publish live again
            // over an issue every reader already had.
            val kPublish = "publish:$pubId"
            val kBill = "bill:$pubId"
            val kReseed = "reseed:$pubId"
            val kCode = "code:$pubId"
            val kMarket = "market:$pubId"
            val kStage = "stage:$pubId"
            var minting by remember(pubId) { mutableStateOf(ThreadSends.inFlight(kCode)) }
            var listing by remember(pubId) { mutableStateOf(ThreadSends.inFlight(kMarket)) }
            // The staging copy is on disk already; its path is what
            // survives, not the File.
            var stagedPath by rememberSaveable(pubId) { mutableStateOf<String?>(null) }
            val tick by ThreadSends.ticks.collectAsState()
            LaunchedEffect(tick, pubId) {
                busy = if (listOf(kPublish, kBill, kReseed).any { ThreadSends.inFlight(it) }) {
                    context.getString(R.string.pub_publishing)
                } else null
                minting = ThreadSends.inFlight(kCode)
                listing = ThreadSends.inFlight(kMarket)
                for (k in listOf(kPublish, kBill, kReseed, kCode, kMarket, kStage)) {
                    for (o in ThreadSends.take(k)) when (o) {
                        is ThreadSends.Outcome.Landed -> when (k) {
                            kPublish -> { stagedPath = null; lastWord = o.result }
                            kCode -> subCode = o.result
                            kStage -> stagedPath = o.result
                            else -> lastWord = o.result
                        }
                        is ThreadSends.Outcome.Failed -> {
                            DucatLog.w("Publishing", "${k.substringBefore(':')}: ${o.error.message}")
                            lastWord = o.error.saidWhy() ?: o.error.javaClass.simpleName
                        }
                    }
                }
            }
            val run: (String, () -> String?) -> Unit = { k, block ->
                ThreadSends.launch(ContactStore(context), k, null) { block() }
            }

            // --- the price, which decides who the mailbag opens for -------
            val storedPrice = remember(version, pubId) { Publications.priceOf(context, pubId) }
            var priceText by rememberSaveable(pubId) {
                mutableStateOf(if (storedPrice > 0) formatXmr(storedPrice) else "")
            }
            // Blank means free; anything else must be a figure. "Set" used
            // to swallow "abc" without a word — now the field says so and
            // the button waits, the way the listing forms do it.
            val pricePxmr = remember(priceText) {
                if (priceText.isBlank()) 0L
                else Amounts.parse(priceText)?.let { Amounts.toPxmr(it) }
            }
            Card(Modifier.fillMaxWidth()) {
                Column(Modifier.padding(12.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        OutlinedTextField(
                            priceText, { priceText = it },
                            label = { Text(stringResource(R.string.pub_price_label)) },
                            singleLine = true,
                            isError = pricePxmr == null,
                            modifier = Modifier.weight(1f),
                        )
                        Spacer(Modifier.width(8.dp))
                        TextButton(
                            enabled = pricePxmr != null,
                            onClick = {
                                pricePxmr?.let { Publications.setPrice(context, pubId, it) }
                            },
                        ) { Text(stringResource(R.string.pub_price_set)) }
                    }
                    Text(
                        stringResource(
                            if (storedPrice > 0) R.string.pub_paid_note
                            else R.string.pub_free_note,
                        ),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(Modifier.height(6.dp))
                    // One card per tap: the mint is a DHT write, and the
                    // button stayed live through it, so a second tap while
                    // the first was still out bound two cards to the
                    // publication and showed only the later one.
                    OutlinedButton(
                        enabled = busy == null && !minting,
                        onClick = {
                            minting = true
                            val title = pubs.firstOrNull { it.first == pubId }?.second
                            run(kCode) {
                                val card = org.ducatproject.ducat.Mailbox.issueCard(
                                    context, title, 7uL * 24uL * 60uL * 60uL,
                                    purpose = "publish",
                                )
                                Publications.bindCard(context, pubId, card.inboxKey)
                                card.uri
                            }
                        },
                    ) { Text(stringResource(R.string.pub_sub_code)) }
                }
            }

            // --- the market: the worldwide shelf (§16.18.2) ---------------
            val mktCat = remember(version, pubId) {
                Publications.marketStateOf(context, pubId)
            }
            var catPick by rememberSaveable(pubId) { mutableStateOf(mktCat?.first ?: "news") }
            var blurbText by rememberSaveable(pubId) { mutableStateOf(mktCat?.second ?: "") }
            Card(Modifier.fillMaxWidth()) {
                Column(Modifier.padding(12.dp)) {
                    Text(
                        stringResource(R.string.market_list_header),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Row(
                        Modifier.horizontalScroll(rememberScrollState()),
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        Publications.MARKET_CATEGORIES.forEach { slug ->
                            FilterChip(
                                selected = catPick == slug,
                                onClick = { catPick = slug },
                                label = { Text(marketCategoryLabel(slug)) },
                            )
                        }
                    }
                    OutlinedTextField(
                        blurbText, { blurbText = it.take(280) },
                        label = { Text(stringResource(R.string.market_blurb_label)) },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Spacer(Modifier.height(6.dp))
                    var alsoLocal by rememberSaveable(pubId) { mutableStateOf(false) }
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Checkbox(alsoLocal, { alsoLocal = it })
                        Text(
                            stringResource(R.string.market_also_local),
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                    // One listing per tap, for the same reason as one card:
                    // the listing is a card mint and a ladder of stand
                    // posts, seconds of DHT, and the button stayed live
                    // through them.
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        OutlinedButton(
                            enabled = busy == null && !listing,
                            onClick = {
                                listing = true
                                val cat = catPick
                                val blurb = blurbText.takeIf { it.isNotBlank() }
                                val local = alsoLocal
                                run(kMarket) {
                                    val lang = java.util.Locale.getDefault().language
                                    // The town paper's own cell, when
                                    // asked for: a fix at click time. The
                                    // worldwide half goes up either way;
                                    // the local half that did not is
                                    // said, not skipped — a ticked box
                                    // and a listing with no cell looked
                                    // like the shelf was broken.
                                    var cell: String? = null
                                    if (local) {
                                        val gate = java.util.concurrent.CountDownLatch(1)
                                        org.ducatproject.ducat.ui.grabFix(context) { fix ->
                                            cell = fix?.let {
                                                runCatching {
                                                    uniffi.ducat_mobile.geohashEncode(
                                                        it.first, it.second,
                                                        org.ducatproject.ducat.Listings.CELL_PRECISION,
                                                    )
                                                }.getOrNull()
                                            }
                                            gate.countDown()
                                        }
                                        gate.await(10, java.util.concurrent.TimeUnit.SECONDS)
                                    }
                                    val ok = runCatching {
                                        Publications.listOnMarket(
                                            context, pubId, cat,
                                            lang.takeIf { it.isNotBlank() }, blurb, cell,
                                        )
                                    }.getOrDefault(false)
                                    org.ducatproject.ducat.ContactStore.bump()
                                    when {
                                        !ok -> context.getString(R.string.market_list_failed)
                                        local && cell == null ->
                                            context.getString(R.string.market_no_fix)
                                        else -> null
                                    }
                                }
                            },
                        ) {
                            if (listing) {
                                CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                            } else {
                                Text(stringResource(R.string.market_list_btn))
                            }
                        }
                        if (mktCat != null) {
                            Spacer(Modifier.width(8.dp))
                            TextButton(onClick = {
                                Publications.delistFromMarket(context, pubId)
                                org.ducatproject.ducat.ContactStore.bump()
                            }) { Text(stringResource(R.string.market_delist_btn)) }
                        }
                    }
                    if (mktCat != null) {
                        Text(
                            stringResource(
                                R.string.market_listed_as,
                                marketCategoryLabel(mktCat.first),
                            ),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.ducat.settled,
                        )
                    }
                }
            }

            // --- the roster, one hat one list -----------------------------
            val personas = remember { PersonaStore(context) }
            val scoped = remember(version) { personas.all().size > 1 }
            val contacts = remember(version) {
                val all = ContactStore(context).all()
                if (scoped) {
                    val worn = personas.worn()
                    all.filter { personas.ownerHexOf(it) == worn }
                } else all
            }
            val roster = remember(version, pubId) {
                Publications.subscribers(context, pubId).toSet()
            }
            Card(Modifier.fillMaxWidth()) {
                Column(Modifier.padding(12.dp)) {
                    Text(
                        stringResource(R.string.pub_roster_title),
                        style = MaterialTheme.typography.titleSmall,
                    )
                    if (contacts.isEmpty()) {
                        Text(
                            stringResource(R.string.pub_roster_empty),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
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
                }
            }

            // --- the issue: period, file, and out the door ----------------
            val now = remember { java.time.YearMonth.now().toString() }
            var period by rememberSaveable(pubId) { mutableStateOf(now) }
            var note by rememberSaveable(pubId) { mutableStateOf("") }
            val staged = stagedPath?.let(::File)?.takeIf { it.isFile }
            val picker = rememberLauncherForActivityResult(
                ActivityResultContracts.OpenDocument(),
            ) { uri ->
                if (uri == null) return@rememberLauncherForActivityResult
                run(kStage) {
                    // The picked name when the platform will say it, a
                    // plain one when it will not — the shelf's index
                    // carries this name to the reader's disk.
                    val name = runCatching {
                        context.contentResolver.query(uri, null, null, null, null)
                            ?.use { cur ->
                                val i = cur.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                                if (cur.moveToFirst() && i >= 0) cur.getString(i) else null
                            }
                    }.getOrNull()
                        ?: uri.lastPathSegment?.substringAfterLast('/')
                        ?: "issue.bin"
                    // One directory per pick. A large issue is served
                    // from where it was staged, for as long as the
                    // publication lives — so a later pick that happened
                    // to share its name, for any publication, was
                    // written over the file a seeder was reading, and
                    // every reader's fetch then failed its digest.
                    val dir = File(
                        context.filesDir,
                        "publish_staging/$pubId/${System.currentTimeMillis()}",
                    ).apply { mkdirs() }
                    val dst = File(dir, name.replace('/', '_'))
                    context.contentResolver.openInputStream(uri)!!.use { input ->
                        dst.outputStream().use { input.copyTo(it) }
                    }
                    dst.absolutePath
                }
            }
            Card(Modifier.fillMaxWidth()) {
                Column(Modifier.padding(12.dp)) {
                    OutlinedTextField(
                        period, { if (it.length <= 64) period = it },
                        label = { Text(stringResource(R.string.pub_period_label)) },
                        singleLine = true,
                    )
                    Spacer(Modifier.height(6.dp))
                    OutlinedTextField(
                        note, { if (it.length <= 500) note = it },
                        label = { Text(stringResource(R.string.pub_note_label)) },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Spacer(Modifier.height(8.dp))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        OutlinedButton(onClick = { picker.launch(arrayOf("*/*")) }) {
                            Text(stringResource(R.string.pub_choose_file))
                        }
                        Spacer(Modifier.width(10.dp))
                        staged?.let {
                            Text(
                                stringResource(
                                    R.string.pub_file_chosen,
                                    it.name,
                                    android.text.format.Formatter
                                        .formatShortFileSize(context, it.length()),
                                ),
                                style = MaterialTheme.typography.labelSmall,
                            )
                        }
                    }
                    Spacer(Modifier.height(8.dp))
                    Button(
                        enabled = busy == null && staged != null &&
                            period.isNotBlank() && roster.isNotEmpty(),
                        onClick = {
                            val f = staged!!
                            val p = period.trim()
                            val n = note.trim()
                            busy = context.getString(R.string.pub_publishing)
                            lastWord = null
                            run(kPublish) {
                                if (f.length() <= Publications.SHELF_MULTI_CAP_BYTES) {
                                    check(
                                        Publications.shelveIssue(context, pubId, p, f),
                                    ) { "the shelf would not take it" }
                                    // The shelf holds the bytes now; the
                                    // staging copy was only ever the way
                                    // there.
                                    f.delete()
                                    f.parentFile?.delete()
                                } else {
                                    val share = Swarm.seed(f.absolutePath)
                                    Publications.recordIssue(
                                        context, pubId, p, f.absolutePath,
                                        share.shareKey, share.indexDigestHex,
                                    )
                                }
                                if (Publications.priceOf(context, pubId) > 0) {
                                    Publications.reconcileSettled(context)
                                    context.getString(R.string.pub_out_paid, p)
                                } else {
                                    val issue = Publications.issues(context, pubId)
                                        .first { it.periodId == p }
                                    val shelf = Publications.shelfOf(context, pubId)
                                    var sent = 0
                                    val readers = ContactStore(context).all().filter {
                                        it.personaHex in
                                            Publications.subscribers(context, pubId)
                                    }
                                    for (c in readers) {
                                        val ok = Publications.sendPeriod(
                                            context, c, pubId, p,
                                            record = shelf?.first,
                                            headKey = shelf?.second,
                                            note = n,
                                            swarmKey = issue.swarmKey
                                                .takeIf { it.isNotBlank() },
                                            swarmDigestHex = issue.swarmDigestHex
                                                .takeIf { it.isNotBlank() },
                                        )
                                        if (ok) {
                                            Publications.markSent(
                                                context, pubId, p, c.personaHex,
                                            )
                                            sent++
                                        }
                                    }
                                    context.getString(R.string.pub_out_free, p, sent)
                                }
                            }
                        },
                    ) { Text(busy ?: stringResource(R.string.pub_publish)) }
                    // A priced roster gets billed from here too — the same
                    // TabStore rails as the till, settlement watched by the
                    // poll clock that already runs.
                    if (storedPrice > 0) {
                        Spacer(Modifier.height(6.dp))
                        val billed = remember(version, pubId, period) {
                            Publications.billedFor(context, pubId, period.trim())
                        }
                        val paid = remember(version, billed) {
                            val tabs = TabStore(context)
                            billed.values.count {
                                tabs.get(it)?.state?.startsWith("paid") == true
                            }
                        }
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            OutlinedButton(
                                enabled = busy == null && roster.isNotEmpty() &&
                                    period.isNotBlank(),
                                onClick = {
                                    busy = context.getString(R.string.pub_publishing)
                                    val p = period.trim()
                                    run(kBill) {
                                        val n = Publications.billPeriod(context, pubId, p)
                                        context.getString(R.string.pub_billed_word, n)
                                    }
                                },
                            ) { Text(stringResource(R.string.pub_bill)) }
                            Spacer(Modifier.width(10.dp))
                            Text(
                                stringResource(
                                    R.string.pub_billed_counts, billed.size, paid,
                                ),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                    lastWord?.let {
                        Spacer(Modifier.height(6.dp))
                        Text(it, style = MaterialTheme.typography.bodySmall)
                    }
                }
            }

            // --- what has shipped -----------------------------------------
            val issues = remember(version, pubId, busy) { Publications.issues(context, pubId) }
            if (issues.isNotEmpty()) {
                Card(Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(12.dp)) {
                        Text(
                            stringResource(R.string.pub_issues_title),
                            style = MaterialTheme.typography.titleSmall,
                        )
                        issues.forEachIndexed { i, issue ->
                            if (i > 0) HorizontalDivider(Modifier.padding(vertical = 6.dp))
                            Row(
                                Modifier.fillMaxWidth().padding(top = 6.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Column(Modifier.weight(1f)) {
                                    Text(
                                        stringResource(R.string.library_issue, issue.periodId),
                                        style = MaterialTheme.typography.bodyLarge,
                                    )
                                    Text(
                                        stringResource(
                                            R.string.pub_issue_row,
                                            issue.sentTo.size,
                                            if (issue.shelfRec.isNotBlank()) {
                                                stringResource(R.string.pub_rail_shelf)
                                            } else {
                                                stringResource(R.string.pub_rail_swarm)
                                            },
                                        ),
                                        style = MaterialTheme.typography.labelSmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                                // Only a swarm month dies with the process; the
                                // shelf holds without us.
                                if (issue.swarmKey.isNotBlank() &&
                                    File(issue.file).isFile
                                ) {
                                    TextButton(
                                        enabled = busy == null,
                                        // Its failure is said (the tick
                                        // effect above): the log alone left
                                        // the button springing back with
                                        // nothing said.
                                        onClick = {
                                            busy = context.getString(R.string.pub_publishing)
                                            run(kReseed) {
                                                val share = Swarm.seed(issue.file)
                                                Publications.recordIssue(
                                                    context, pubId, issue.periodId,
                                                    issue.file, share.shareKey,
                                                    share.indexDigestHex,
                                                )
                                                val readers = ContactStore(context)
                                                    .all()
                                                    .filter {
                                                        it.personaHex in issue.sentTo
                                                    }
                                                for (c in readers) {
                                                    Publications.sendPeriod(
                                                        context, c, pubId,
                                                        issue.periodId,
                                                        record = null, headKey = null,
                                                        note = "",
                                                        swarmKey = share.shareKey,
                                                        swarmDigestHex =
                                                            share.indexDigestHex,
                                                    )
                                                }
                                                null
                                            }
                                        },
                                    ) { Text(stringResource(R.string.pub_reseed)) }
                                }
                            }
                        }
                    }
                }
            }

            // --- the end of the run ---------------------------------------
            var askDelete by remember(pubId) { mutableStateOf(false) }
            TextButton(
                onClick = { askDelete = true },
                colors = ButtonDefaults.textButtonColors(
                    contentColor = MaterialTheme.colorScheme.error,
                ),
            ) { Text(stringResource(R.string.pub_delete_btn)) }
            if (askDelete) {
                AlertDialog(
                    onDismissRequest = { askDelete = false },
                    title = { Text(stringResource(R.string.pub_delete_title)) },
                    text = { Text(stringResource(R.string.pub_delete_body)) },
                    confirmButton = {
                        TextButton(onClick = {
                            askDelete = false
                            Publications.deletePub(context, pubId)
                        }) {
                            Text(
                                stringResource(R.string.pub_delete_confirm),
                                color = MaterialTheme.colorScheme.error,
                            )
                        }
                    },
                    dismissButton = {
                        TextButton(onClick = { askDelete = false }) {
                            Text(stringResource(R.string.common_cancel))
                        }
                    },
                )
            }
        }
    }
}
