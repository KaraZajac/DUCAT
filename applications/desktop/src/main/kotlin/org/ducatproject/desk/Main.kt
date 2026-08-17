package org.ducatproject.desk

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.toComposeImageBitmap
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Window
import androidx.compose.ui.window.application
import com.google.zxing.BarcodeFormat
import com.google.zxing.qrcode.QRCodeWriter
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Balances
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.NameStore
import org.ducatproject.ducat.NodeStore
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.Rates
import org.ducatproject.ducat.StoredMessage
import org.ducatproject.ducat.Wallet
import org.ducatproject.ducat.WalletStore
import org.ducatproject.ducat.formatXmr
import java.io.File
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus

/**
 * DUCAT Desk v3 — the phone's protocol brain with a desk's face.
 *
 * Everything below the UI is the *same compiled source* the Android app
 * ships: Mailbox's patience windows and atomic sends, ContactStore's
 * partitioned prekeys, the wallet's plan/quote/send, the chain rules. The
 * desk adds a window: contacts on the left, the thread on the right, a card
 * on screen for a phone to claim — a wallet that scans, bills that render,
 * a Pay that quotes before it spends, receipts, a tray bell.
 *
 * The window's language is a shopkeeper's, on purpose: "online", "checking
 * for new payments", names instead of hex. Peer counts, block heights and
 * node errors exist — one click away, behind the status word — because the
 * person at the till needs "is it working", not a telemetry feed.
 */

private fun dataDir(): File {
    // DUCAT_DESK_STATE names the identity: two desks on one machine are two
    // directories, each a complete persona/wallet/contacts. What must never
    // happen is two *processes* on one directory — see lockOrExplain.
    System.getenv("DUCAT_DESK_STATE")?.takeIf { it.isNotEmpty() }?.let {
        return File(it).apply { mkdirs() }
    }
    val base = System.getenv("XDG_DATA_HOME")?.takeIf { it.isNotEmpty() }
        ?: "${System.getProperty("user.home")}/.local/share"
    return File(base, "ducat-desk").apply { mkdirs() }
}

/**
 * One process per identity, enforced. Two desks on one state dir would race
 * the Veilid store and tear the chain counters and prekey burns — the state
 * a week of field fixes made crash-proof, corrupted by its own twin. The
 * lock is held for the process's life; the OS releases it on any death.
 */
private fun lockOrExplain(dir: File): java.nio.channels.FileLock? {
    val ch = java.io.RandomAccessFile(File(dir, "desk.lock"), "rw").channel
    return ch.tryLock()
}

private fun qrBitmap(text: String, px: Int = 380): ImageBitmap {
    val m = QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, px, px)
    val img = java.awt.image.BufferedImage(px, px, java.awt.image.BufferedImage.TYPE_INT_RGB)
    for (x in 0 until px) for (y in 0 until px) {
        img.setRGB(x, y, if (m.get(x, y)) 0x000000 else 0xFFFFFF)
    }
    return img.toComposeImageBitmap()
}

/** The same first-usable-node probe the phone's poller runs. */
private fun pickNode(context: android.content.Context): String? = runCatching {
    val store = NodeStore(context)
    val s = uniffi.ducat_mobile.moneroPickNode(
        uniffi.ducat_mobile.moneroDefaultNodes(store.ownUrl()),
        "stagenet",
        8_000u,
    )
    store.rememberLastGood(s.url)
    s.url
}.getOrNull()

/**
 * A tray notification, if this desktop offers a tray; silence if not. The
 * desk is a till — "a bill arrived while you were making coffee" is the
 * one interruption it owes its operator.
 */
private object DeskTray {
    private val icon: java.awt.TrayIcon? by lazy {
        runCatching {
            if (!java.awt.SystemTray.isSupported()) return@runCatching null
            val img = java.awt.image.BufferedImage(16, 16, java.awt.image.BufferedImage.TYPE_INT_ARGB)
            img.createGraphics().apply {
                color = java.awt.Color(0xF5, 0xA9, 0x7F)
                fillOval(1, 1, 14, 14)
                dispose()
            }
            java.awt.TrayIcon(img, "DUCAT Desk").also {
                it.isImageAutoSize = true
                java.awt.SystemTray.getSystemTray().add(it)
            }
        }.getOrNull()
    }

    fun post(title: String, body: String) {
        runCatching {
            icon?.displayMessage(title, body, java.awt.TrayIcon.MessageType.NONE)
        }
    }
}

/** One line of what a message *is*, for the thread and the tray alike. */
private fun StoredMessage.headline(): String = when (kind) {
    1 -> "asks for ${formatXmr(amountPxmr)} XMR"
    2 -> "sent ${formatXmr(amountPxmr)} XMR"
    3 -> "receipt for ${formatXmr(amountPxmr)} XMR" + if (oob) " — settled outside DUCAT" else ""
    5 -> "withdrew a message"
    6 -> "offers to drive — ${formatXmr(amountPxmr)} XMR" +
        (etaSecs?.let { ", ${it / 60} min away" } ?: "")
    7 -> "ride accepted — ${formatXmr(amountPxmr)} XMR"
    else -> ""
}

/** "≈ USD 1.23", when a rate exists; silence when it does not. */
private fun fiatOf(context: DeskContext, pxmr: Long): String? =
    runCatching {
        Rates.view(context, pxmr, WalletStore(context).stagenet())?.let { "≈ ${it.text}" }
    }.getOrNull()

private fun clock(ts: Long): String =
    java.time.LocalTime.ofInstant(
        java.time.Instant.ofEpochSecond(ts), java.time.ZoneId.systemDefault(),
    ).let { "%02d:%02d".format(it.hour, it.minute) }

fun main() {
    // Packaged, the native library travels inside the distribution; the
    // launcher exports where. In dev the gradle task points at
    // ../target/release instead, and this property is simply absent.
    System.getProperty("compose.application.resources.dir")?.let {
        System.setProperty("jna.library.path", it)
    }
    val dir = dataDir()
    if (lockOrExplain(dir) == null) {
        System.err.println(
            "ducat-desk: ${dir.absolutePath} is already in use by another desk.\n" +
            "Two processes on one identity would corrupt it. For a second desk,\n" +
            "give it its own: DUCAT_DESK_STATE=~/path/to/other-desk",
        )
        kotlin.system.exitProcess(1)
    }
    runDesk(dir)
}

private fun runDesk(deskDir: File) = application {
    Window(onCloseRequest = ::exitApplication, title = "DUCAT Desk") {
        val context = remember { DeskContext(deskDir) }
        var ready by remember { mutableStateOf(false) }
        var netWord by remember { mutableStateOf("starting…") }
        var netDetail by remember { mutableStateOf("") }
        var netOpen by remember { mutableStateOf(false) }
        var contacts by remember { mutableStateOf<List<Contact>>(emptyList()) }
        var unread by remember { mutableStateOf<Set<String>>(emptySet()) }
        var selected by remember { mutableStateOf<String?>(null) }
        var thread by remember { mutableStateOf<List<StoredMessage>>(emptyList()) }
        var cardUri by remember { mutableStateOf<String?>(null) }
        var draft by remember { mutableStateOf("") }
        var error by remember { mutableStateOf<String?>(null) }
        var balances by remember { mutableStateOf<Balances?>(null) }
        var fiat by remember { mutableStateOf<String?>(null) }
        var deskName by remember { mutableStateOf<String?>(null) }
        var renameOpen by remember { mutableStateOf(false) }
        var receiveOpen by remember { mutableStateOf(false) }
        var payFor by remember { mutableStateOf<StoredMessage?>(null) }
        var focused by remember { mutableStateOf(true) }

        // The tray only speaks for messages that land while the operator is
        // elsewhere — a focus listener on the real AWT frame decides, and the
        // shared Mailbox's announce funnel (DeskGlue's Notify) delivers.
        DisposableEffect(Unit) {
            val l = object : java.awt.event.WindowFocusListener {
                override fun windowGainedFocus(e: java.awt.event.WindowEvent?) { focused = true }
                override fun windowLostFocus(e: java.awt.event.WindowEvent?) { focused = false }
            }
            window.addWindowFocusListener(l)
            org.ducatproject.ducat.Notify.sink = { from, personaHex, m ->
                if (!focused || selected != personaHex) {
                    DeskTray.post(from, m.headline().ifEmpty { m.body.take(80) })
                }
            }
            onDispose {
                org.ducatproject.ducat.Notify.sink = null
                window.removeWindowFocusListener(l)
            }
        }

        // The node, then the poller: the same loop the phone's service runs,
        // with the wallet's scan folded in beside the mailbox sweep.
        LaunchedEffect(Unit) {
            deskName = runCatching { MyProfile(context).name() }.getOrNull()
            withContext(Dispatchers.IO) {
                runCatching { nodeStart(File(deskDir, "veilid").absolutePath, true) }
                    .onFailure { error = "The network could not start: ${it.message}" }
                // A desk born without a wallet mints one now, exactly as
                // onboarding does: creation height from a live node so the
                // scan starts at today instead of genesis.
                if (WalletStore(context).address() == null) {
                    runCatching {
                        val tip = runCatching {
                            uniffi.ducat_mobile.moneroPickNode(
                                uniffi.ducat_mobile.moneroDefaultNodes(null), "stagenet", 8_000u,
                            ).height
                        }.getOrDefault(0uL)
                        val w = uniffi.ducat_mobile.createWallet(tipHeight = tip, stagenet = true)
                        WalletStore(context).save(w.address, w.spendKeyHex, w.restoreHeight, true)
                    }.onFailure { error = "The wallet could not be created: ${it.message}" }
                }
            }
            var tick = 0L
            while (true) {
                val s = runCatching { nodeStatus() }.getOrNull()
                // One word for the till; the numbers wait behind a click.
                netWord = when {
                    s?.publicInternetReady == true -> "online"
                    s != null -> "connecting…"
                    else -> "starting…"
                }
                netDetail = buildString {
                    append("Network: ")
                    append(
                        if (s == null) "starting"
                        else "${if (s.publicInternetReady) "connected" else "attaching"} — " +
                            "${s.reliablePeers}/${s.peers} peers",
                    )
                    balances?.let {
                        append("\nWallet: scanned to block ${it.scannedTo} of ${it.tip}")
                        it.error?.let { e -> append("\nLast scan problem: $e") }
                    }
                    NodeStore(context).lastGood()?.let { append("\nMonero node: $it") }
                }
                if (s?.publicInternetReady == true && !ready) ready = true
                if (ready) {
                    withContext(Dispatchers.IO) {
                        runCatching {
                            Mailbox.collectClaims(context)
                            Mailbox.poll(context)
                        }
                        // The wallet keeps pace beside the mailbox: a few
                        // scan windows a tick, so a syncing desk converges
                        // without starving the poll.
                        runCatching {
                            if (WalletStore(context).address() != null) {
                                val node = NodeStore(context).lastGood() ?: pickNode(context)
                                if (node != null) {
                                    var steps = 0
                                    while (steps < 3 && Wallet.scanStep(context, node)) steps++
                                }
                                balances = Wallet.balances(context)
                                fiat = balances?.let { fiatOf(context, it.spendablePxmr) }
                            }
                        }
                        if (tick % 225L == 0L) runCatching { Rates.refresh(context) }
                    }
                    val store = ContactStore(context)
                    contacts = store.all().sortedBy { it.displayName().lowercase() }
                    unread = contacts
                        .filter { it.inSeq > store.chatSeen(it.personaHex) }
                        .map { it.personaHex }.toSet()
                    selected?.let { sel ->
                        thread = store.thread(sel)
                        // Watching the thread is reading it.
                        contacts.firstOrNull { it.personaHex == sel }
                            ?.let { store.setChatSeen(sel, it.inSeq) }
                    }
                }
                tick++
                delay(4_000)
            }
        }

        MaterialTheme(colorScheme = darkColorScheme()) {
            Surface(Modifier.fillMaxSize()) {
                Column(Modifier.fillMaxSize()) {
                    // Top bar: who this desk is, one status word, what it holds.
                    Row(
                        Modifier.fillMaxWidth().padding(12.dp),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        TextButton(onClick = { renameOpen = true }) {
                            Text(
                                deskName ?: "DUCAT Desk",
                                style = MaterialTheme.typography.titleLarge,
                            )
                        }
                        Column(horizontalAlignment = Alignment.CenterHorizontally) {
                            TextButton(onClick = { netOpen = true }) { Text(netWord) }
                            balances?.let { b ->
                                val money = buildString {
                                    fiat?.let { append("$it · ") }
                                    append("${formatXmr(b.spendablePxmr)} XMR")
                                    if (b.lockedPxmr > 0) {
                                        append(" (+${formatXmr(b.lockedPxmr)} arriving)")
                                    }
                                    if (b.syncing) append(" · checking for new payments…")
                                }
                                Text(money, style = MaterialTheme.typography.bodySmall)
                            }
                        }
                        Row {
                            OutlinedButton(
                                enabled = WalletStore(context).address() != null,
                                onClick = { receiveOpen = true },
                            ) { Text("Receive") }
                            Spacer(Modifier.width(8.dp))
                            Button(
                                enabled = ready,
                                onClick = {
                                    cardUri = runCatching {
                                        Mailbox.issueCard(
                                            context, MyProfile(context).name() ?: "desk",
                                            (24uL * 60uL * 60uL),
                                        ).uri
                                    }.getOrNull()
                                },
                            ) { Text("My card") }
                        }
                    }
                    HorizontalDivider()

                    Row(Modifier.fillMaxSize()) {
                        // Contacts, the unread marked.
                        LazyColumn(Modifier.width(230.dp).fillMaxHeight()) {
                            items(contacts, key = { it.personaHex }) { c ->
                                val here = c.personaHex == selected
                                TextButton(
                                    onClick = {
                                        selected = c.personaHex
                                        val store = ContactStore(context)
                                        thread = store.thread(c.personaHex)
                                        store.setChatSeen(c.personaHex, c.inSeq)
                                        unread = unread - c.personaHex
                                    },
                                    modifier = Modifier.fillMaxWidth(),
                                ) {
                                    val mark = when {
                                        here -> "▸ "
                                        c.personaHex in unread -> "● "
                                        else -> ""
                                    }
                                    Text(
                                        mark + c.displayName(),
                                        style = MaterialTheme.typography.bodyMedium,
                                    )
                                }
                            }
                            if (contacts.isEmpty()) {
                                item {
                                    Text(
                                        "nobody yet —\nshow a card to a phone",
                                        Modifier.padding(16.dp),
                                        style = MaterialTheme.typography.bodySmall,
                                    )
                                }
                            }
                        }
                        VerticalDivider()

                        // The thread.
                        Column(Modifier.weight(1f).fillMaxHeight().padding(12.dp)) {
                            val listState = rememberLazyListState()
                            // Ceremony rounds are machinery, not conversation.
                            val visible = thread.filter { it.kind !in 8..10 }
                            LaunchedEffect(visible.size) {
                                if (visible.isNotEmpty()) listState.scrollToItem(visible.size - 1)
                            }
                            LazyColumn(Modifier.weight(1f), state = listState) {
                                items(visible) { m ->
                                    MessageRow(
                                        context = context,
                                        m = m,
                                        thread = thread,
                                        onPay = { payFor = it },
                                        onReceipt = { paid ->
                                            val to = selected ?: return@MessageRow
                                            Thread {
                                                runCatching {
                                                    val store = ContactStore(context)
                                                    val c = store.all().first { it.personaHex == to }
                                                    Mailbox.send(
                                                        context, c, "Receipt — thank you",
                                                        PersonaStore(context).personaHex(),
                                                        kind = 3, amountPxmr = paid.amountPxmr,
                                                        txidHex = paid.txidHex,
                                                    )
                                                }.onFailure {
                                                    error = "The receipt did not go out: ${it.message}"
                                                }
                                            }.start()
                                        },
                                    )
                                }
                            }
                            error?.let {
                                Row(verticalAlignment = Alignment.CenterVertically) {
                                    Text(
                                        it, Modifier.weight(1f),
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.error,
                                    )
                                    TextButton(onClick = { error = null }) { Text("dismiss") }
                                }
                            }
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                OutlinedTextField(
                                    value = draft,
                                    onValueChange = { draft = it },
                                    modifier = Modifier.weight(1f),
                                    placeholder = { Text("Message") },
                                    singleLine = true,
                                )
                                Spacer(Modifier.width(8.dp))
                                Button(
                                    enabled = draft.isNotBlank() && selected != null,
                                    onClick = {
                                        val to = selected ?: return@Button
                                        val text = draft
                                        draft = ""
                                        Thread {
                                            runCatching {
                                                val store = ContactStore(context)
                                                val c = store.all().first { it.personaHex == to }
                                                Mailbox.send(
                                                    context, c, text,
                                                    PersonaStore(context).personaHex(),
                                                )
                                            }.onFailure {
                                                error = "The message did not go out: ${it.message}"
                                            }
                                        }.start()
                                    },
                                ) { Text("Send") }
                            }
                        }
                    }
                }

                // The card, full screen: a phone at the desk scans this and
                // the claim lands in the poller like any other. The link is a
                // copy button, not a wall of base64 — it is for pasting into
                // a chat app, not for reading.
                cardUri?.let { uri ->
                    val clipboard = LocalClipboardManager.current
                    AlertDialog(
                        onDismissRequest = { cardUri = null },
                        confirmButton = {
                            TextButton(onClick = { cardUri = null }) { Text("Done") }
                        },
                        dismissButton = {
                            TextButton(
                                onClick = { clipboard.setText(AnnotatedString(uri)) },
                            ) { Text("Copy link") }
                        },
                        title = { Text("Scan to connect") },
                        text = {
                            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                                Image(qrBitmap(uri), contentDescription = "contact card")
                                Spacer(Modifier.height(8.dp))
                                Text(
                                    "Good for 24 hours, one claim.",
                                    style = MaterialTheme.typography.bodySmall,
                                )
                            }
                        },
                    )
                }

                // Give-to-this-desk: the wallet's standing address. One
                // address, every giver can see it is the same till — for a
                // till, that is the point (§16.12 stated its cost).
                if (receiveOpen) {
                    val addr = WalletStore(context).address() ?: ""
                    val clipboard = LocalClipboardManager.current
                    AlertDialog(
                        onDismissRequest = { receiveOpen = false },
                        confirmButton = {
                            TextButton(onClick = { receiveOpen = false }) { Text("Done") }
                        },
                        dismissButton = {
                            TextButton(
                                onClick = { clipboard.setText(AnnotatedString(addr)) },
                            ) { Text("Copy address") }
                        },
                        title = { Text("Pay this desk") },
                        text = {
                            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                                Image(qrBitmap(addr), contentDescription = "wallet address")
                                Spacer(Modifier.height(8.dp))
                                Text(
                                    "${addr.take(12)}…${addr.takeLast(6)}",
                                    style = MaterialTheme.typography.bodySmall,
                                )
                            }
                        },
                    )
                }

                // Naming the desk names the cards it hands out.
                if (renameOpen) {
                    var name by remember { mutableStateOf(deskName ?: "") }
                    AlertDialog(
                        onDismissRequest = { renameOpen = false },
                        confirmButton = {
                            TextButton(
                                enabled = name.isNotBlank(),
                                onClick = {
                                    runCatching { NameStore(context).put(name.trim()) }
                                    deskName = name.trim()
                                    renameOpen = false
                                },
                            ) { Text("Save") }
                        },
                        dismissButton = {
                            TextButton(onClick = { renameOpen = false }) { Text("Cancel") }
                        },
                        title = { Text("What should people call this desk?") },
                        text = {
                            OutlinedTextField(
                                value = name,
                                onValueChange = { name = it },
                                singleLine = true,
                                placeholder = { Text("Corner Café") },
                            )
                        },
                    )
                }

                // The numbers, for whoever clicks the status word.
                if (netOpen) {
                    AlertDialog(
                        onDismissRequest = { netOpen = false },
                        confirmButton = {
                            TextButton(onClick = { netOpen = false }) { Text("Done") }
                        },
                        title = { Text("Connection") },
                        text = { Text(netDetail.ifEmpty { "starting…" }) },
                    )
                }

                // Pay: quoted before signed, §5's review made a desk dialog.
                payFor?.let { req ->
                    PayDialog(
                        context = context,
                        req = req,
                        contact = contacts.firstOrNull { it.personaHex == selected },
                        onDone = { payFor = null },
                    )
                }
            }
        }
    }
}

/**
 * One message, rendered as what it is. Bills carry their lines (already
 * proven to sum — core refused them otherwise), requests carry a Pay that
 * quotes first, incoming payments offer the receipt this desk owes. What
 * does not appear: transaction hex, sequence numbers, ceremony bytes — the
 * thread is a conversation, and the protocol keeps its own books.
 */
@Composable
private fun MessageRow(
    context: DeskContext,
    m: StoredMessage,
    thread: List<StoredMessage>,
    onPay: (StoredMessage) -> Unit,
    onReceipt: (StoredMessage) -> Unit,
) {
    val who = if (m.outgoing) "me" else "them"
    val head = m.headline()
    val fiat = if (m.amountPxmr > 0) fiatOf(context, m.amountPxmr) else null
    Column(Modifier.fillMaxWidth().padding(vertical = 3.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                if (head.isEmpty()) "$who: ${m.body}"
                else "$who: $head" + (fiat?.let { " ($it)" } ?: ""),
                Modifier.weight(1f),
                style = MaterialTheme.typography.bodyMedium,
            )
            Text(clock(m.timestamp), style = MaterialTheme.typography.labelSmall)
        }
        if (head.isNotEmpty() && m.body.isNotBlank() && m.kind != 5) {
            Text("  ${m.body}", style = MaterialTheme.typography.bodySmall)
        }
        if (m.items.isNotEmpty()) {
            m.items.forEach {
                Text(
                    "    ${it.description} — ${formatXmr(it.amountPxmr)}",
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            m.taxPxmr?.takeIf { it > 0 }?.let {
                Text("    tax — ${formatXmr(it)}", style = MaterialTheme.typography.bodySmall)
            }
        }
        when {
            // An incoming request that names where to pay, and that the
            // sender has not since withdrawn (§16.14 re_own on its seq).
            m.kind == 1 && !m.outgoing && m.payto != null -> {
                val cancelled = thread.any {
                    !it.outgoing && it.kind == 5 && it.reOwn && it.reSeq == m.seq
                }
                if (cancelled) {
                    Text("    cancelled by them", style = MaterialTheme.typography.bodySmall)
                } else {
                    TextButton(onClick = { onPay(m) }) { Text("Pay ${formatXmr(m.amountPxmr)} XMR") }
                }
            }
            // An incoming payment: the receipt is the payee's to give, once.
            m.kind == 2 && !m.outgoing -> {
                val receipted = thread.any {
                    it.outgoing && it.kind == 3 &&
                        (it.txidHex == m.txidHex || (m.txidHex == null && it.amountPxmr == m.amountPxmr))
                }
                if (receipted) {
                    Text("    receipted ✓", style = MaterialTheme.typography.bodySmall)
                } else {
                    TextButton(onClick = { onReceipt(m) }) { Text("Send receipt") }
                }
            }
        }
    }
}

/**
 * The §5 rule, desk-shaped: a request is *reviewed*, never one-tap paid.
 * The dialog is the review — who, how much, the fee, what remains — and
 * the button under it is the only thing that spends.
 */
@Composable
private fun PayDialog(
    context: DeskContext,
    req: StoredMessage,
    contact: Contact?,
    onDone: () -> Unit,
) {
    var quote by remember { mutableStateOf<org.ducatproject.ducat.Quote?>(null) }
    var busy by remember { mutableStateOf(false) }
    var payErr by remember { mutableStateOf<String?>(null) }
    var sent by remember { mutableStateOf(false) }
    LaunchedEffect(req.seq) {
        withContext(Dispatchers.IO) {
            quote = runCatching { Wallet.quote(context, req.amountPxmr) }.getOrNull()
        }
    }
    AlertDialog(
        onDismissRequest = { if (!busy) onDone() },
        title = {
            Text(
                if (sent) "Paid"
                else "Pay ${contact?.displayName() ?: "them"} ${formatXmr(req.amountPxmr)} XMR",
            )
        },
        text = {
            Column {
                if (sent) {
                    Text("Done — they have been told in the thread.")
                    return@Column
                }
                fiatOf(context, req.amountPxmr)?.let {
                    Text(it, style = MaterialTheme.typography.bodySmall)
                }
                if (req.body.isNotBlank()) {
                    Text("for: ${req.body}", style = MaterialTheme.typography.bodySmall)
                }
                Spacer(Modifier.height(6.dp))
                quote?.let { q ->
                    Text("network fee ${formatXmr(q.feePxmr)} XMR")
                    Text(
                        "leaves ${formatXmr(q.remainingPxmr)} XMR in the wallet",
                        style = MaterialTheme.typography.bodySmall,
                    )
                    if (!q.affordable) {
                        Text(
                            "not enough in the wallet yet",
                            color = MaterialTheme.colorScheme.error,
                        )
                    }
                } ?: Text("working out the fee…", style = MaterialTheme.typography.bodySmall)
                payErr?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        },
        confirmButton = {
            if (!sent) {
                Button(
                    enabled = !busy && quote?.affordable == true && req.payto != null,
                    onClick = {
                        busy = true
                        payErr = null
                        Thread {
                            runCatching {
                                val node = NodeStore(context).lastGood() ?: pickNode(context)
                                    ?: throw IllegalStateException("no Monero node reachable")
                                val res = Wallet.send(
                                    context, node, req.payto!!, req.amountPxmr,
                                    contactHex = contact?.personaHex,
                                    note = null,
                                )
                                // §16.13's notice: names the transaction so
                                // their wallet can put this desk's name on the
                                // arriving output. Monero carries no sender.
                                contact?.let { c ->
                                    runCatching {
                                        Mailbox.send(
                                            context, c, "Payment",
                                            PersonaStore(context).personaHex(),
                                            kind = 2, amountPxmr = req.amountPxmr,
                                            txidHex = res.txidHex,
                                        )
                                    }
                                }
                                sent = true
                            }.onFailure { payErr = "The payment did not go through: ${it.message}" }
                            busy = false
                        }.start()
                    },
                ) { Text(if (busy) "Sending…" else "Confirm and send") }
            } else {
                TextButton(onClick = onDone) { Text("Done") }
            }
        },
        dismissButton = {
            if (!sent) {
                TextButton(enabled = !busy, onClick = onDone) { Text("Cancel") }
            }
        },
    )
}
