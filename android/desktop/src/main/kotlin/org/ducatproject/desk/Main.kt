package org.ducatproject.desk

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.toComposeImageBitmap
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
 * on screen for a phone to claim — and now a wallet that scans, bills that
 * render as bills, a Pay that quotes before it spends, receipts the desk
 * can issue, and a tray notification when a message lands unwatched.
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
    5 -> "withdrew message ${reSeq ?: "?"}"
    6 -> "offers to drive — ${formatXmr(amountPxmr)} XMR" +
        (etaSecs?.let { ", ${it / 60} min away" } ?: "")
    7 -> "ride accepted — ${formatXmr(amountPxmr)} XMR"
    else -> ""
}

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
    Window(onCloseRequest = ::exitApplication, title = "DUCAT Desk — ${deskDir.name}") {
        val context = remember { DeskContext(deskDir) }
        var ready by remember { mutableStateOf(false) }
        var statusLine by remember { mutableStateOf("starting…") }
        var contacts by remember { mutableStateOf<List<Contact>>(emptyList()) }
        var selected by remember { mutableStateOf<String?>(null) }
        var thread by remember { mutableStateOf<List<StoredMessage>>(emptyList()) }
        var cardUri by remember { mutableStateOf<String?>(null) }
        var draft by remember { mutableStateOf("") }
        var error by remember { mutableStateOf<String?>(null) }
        var balances by remember { mutableStateOf<Balances?>(null) }
        var fiat by remember { mutableStateOf<String?>(null) }
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
            withContext(Dispatchers.IO) {
                runCatching { nodeStart(File(deskDir, "veilid").absolutePath, true) }
                    .onFailure { error = it.message }
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
                    }.onFailure { error = "wallet: ${it.message}" }
                }
            }
            var tick = 0L
            while (true) {
                val s = runCatching { nodeStatus() }.getOrNull()
                statusLine = when {
                    error != null -> "node: $error"
                    s == null -> "node: starting…"
                    s.publicInternetReady -> "ready — ${s.reliablePeers}/${s.peers} peers"
                    else -> "attaching… (${s?.peers ?: 0u} peers)"
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
                                fiat = balances?.let {
                                    Rates.view(context, it.spendablePxmr, WalletStore(context).stagenet())?.text
                                }
                            }
                        }
                        if (tick % 225L == 0L) runCatching { Rates.refresh(context) }
                    }
                    val store = ContactStore(context)
                    contacts = store.all().sortedBy { it.displayName().lowercase() }
                    selected?.let { thread = store.thread(it) }
                }
                tick++
                delay(4_000)
            }
        }

        MaterialTheme(colorScheme = darkColorScheme()) {
            Surface(Modifier.fillMaxSize()) {
                Column(Modifier.fillMaxSize()) {
                    // Top bar: who this desk is, how connected, and what it holds.
                    Row(
                        Modifier.fillMaxWidth().padding(12.dp),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text("DUCAT Desk", style = MaterialTheme.typography.titleLarge)
                        Column(horizontalAlignment = Alignment.CenterHorizontally) {
                            Text(statusLine, style = MaterialTheme.typography.bodySmall)
                            balances?.let { b ->
                                val syncing =
                                    if (b.syncing) " · scanning ${b.scannedTo}/${b.tip}" else ""
                                val locked =
                                    if (b.lockedPxmr > 0) " (+${formatXmr(b.lockedPxmr)} arriving)" else ""
                                Text(
                                    "${formatXmr(b.spendablePxmr)} XMR$locked" +
                                        (fiat?.let { " · $it" } ?: "") + syncing,
                                    style = MaterialTheme.typography.bodySmall,
                                )
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
                        // Contacts.
                        LazyColumn(Modifier.width(230.dp).fillMaxHeight()) {
                            items(contacts, key = { it.personaHex }) { c ->
                                val here = c.personaHex == selected
                                TextButton(
                                    onClick = {
                                        selected = c.personaHex
                                        thread = ContactStore(context).thread(c.personaHex)
                                    },
                                    modifier = Modifier.fillMaxWidth(),
                                ) {
                                    Text(
                                        (if (here) "▸ " else "") + c.displayName(),
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
                                                }.onFailure { error = it.message }
                                            }.start()
                                        },
                                    )
                                }
                            }
                            error?.let {
                                Text(
                                    it, style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.error,
                                )
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
                                            }.onFailure { error = it.message }
                                        }.start()
                                    },
                                ) { Text("Send") }
                            }
                        }
                    }
                }

                // The card, full screen: a phone at the desk scans this and
                // the claim lands in the poller like any other.
                cardUri?.let { uri ->
                    AlertDialog(
                        onDismissRequest = { cardUri = null },
                        confirmButton = {
                            TextButton(onClick = { cardUri = null }) { Text("Done") }
                        },
                        title = { Text("Scan to connect") },
                        text = {
                            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                                Image(qrBitmap(uri), contentDescription = "contact card")
                                Spacer(Modifier.height(8.dp))
                                SelectionContainer {
                                    Text(uri, style = MaterialTheme.typography.bodySmall, maxLines = 3)
                                }
                            }
                        },
                    )
                }

                // Give-to-this-desk: the wallet's standing address. §16.12's
                // linkability cost is the donate screen's lesson — one address,
                // every giver can see it is the same till. For a till, that is
                // the point.
                if (receiveOpen) {
                    val addr = WalletStore(context).address() ?: ""
                    AlertDialog(
                        onDismissRequest = { receiveOpen = false },
                        confirmButton = {
                            TextButton(onClick = { receiveOpen = false }) { Text("Done") }
                        },
                        title = { Text("Pay this desk") },
                        text = {
                            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                                Image(qrBitmap(addr), contentDescription = "wallet address")
                                Spacer(Modifier.height(8.dp))
                                SelectionContainer {
                                    Text(addr, style = MaterialTheme.typography.bodySmall, maxLines = 4)
                                }
                            }
                        },
                    )
                }

                // Pay: quoted before signed, §5's review made a desk dialog.
                payFor?.let { req ->
                    PayDialog(
                        context = context,
                        req = req,
                        contactHex = selected,
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
 * quotes first, incoming payments offer the receipt this desk owes.
 */
@Composable
private fun MessageRow(
    m: StoredMessage,
    thread: List<StoredMessage>,
    onPay: (StoredMessage) -> Unit,
    onReceipt: (StoredMessage) -> Unit,
) {
    val who = if (m.outgoing) "me" else "them"
    val head = m.headline()
    Column(Modifier.fillMaxWidth().padding(vertical = 3.dp)) {
        Text(
            if (head.isEmpty()) "$who: ${m.body}" else "$who: $head",
            style = MaterialTheme.typography.bodyMedium,
        )
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
        m.txidHex?.let {
            Text("    tx ${it.take(12)}…", style = MaterialTheme.typography.bodySmall)
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
 * The dialog is the review — amount, fee, what remains — and the button
 * under it is the only thing that spends.
 */
@Composable
private fun PayDialog(
    context: DeskContext,
    req: StoredMessage,
    contactHex: String?,
    onDone: () -> Unit,
) {
    var quote by remember { mutableStateOf<org.ducatproject.ducat.Quote?>(null) }
    var busy by remember { mutableStateOf(false) }
    var payErr by remember { mutableStateOf<String?>(null) }
    var sentTx by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(req.seq) {
        withContext(Dispatchers.IO) {
            quote = runCatching { Wallet.quote(context, req.amountPxmr) }.getOrNull()
        }
    }
    AlertDialog(
        onDismissRequest = { if (!busy) onDone() },
        title = { Text(if (sentTx == null) "Pay ${formatXmr(req.amountPxmr)} XMR" else "Paid") },
        text = {
            Column {
                sentTx?.let {
                    Text("tx ${it.take(16)}… — they have been told in the thread")
                    return@Column
                }
                Text("to ${req.payto?.take(24)}…", style = MaterialTheme.typography.bodySmall)
                quote?.let { q ->
                    Text("fee ${formatXmr(q.feePxmr)} XMR · total ${formatXmr(q.totalPxmr)} XMR")
                    Text(
                        "leaves ${formatXmr(q.remainingPxmr)} XMR unlocked",
                        style = MaterialTheme.typography.bodySmall,
                    )
                    if (!q.affordable) {
                        Text("not enough unlocked", color = MaterialTheme.colorScheme.error)
                    }
                } ?: Text("quoting…", style = MaterialTheme.typography.bodySmall)
                payErr?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        },
        confirmButton = {
            if (sentTx == null) {
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
                                    contactHex = contactHex,
                                    note = null,
                                )
                                // §16.13's notice: names the transaction so
                                // their wallet can put this desk's name on the
                                // arriving output. Monero carries no sender.
                                contactHex?.let { hex ->
                                    runCatching {
                                        val store = ContactStore(context)
                                        val c = store.all().first { it.personaHex == hex }
                                        Mailbox.send(
                                            context, c, "Payment",
                                            PersonaStore(context).personaHex(),
                                            kind = 2, amountPxmr = req.amountPxmr,
                                            txidHex = res.txidHex,
                                        )
                                    }
                                }
                                sentTx = res.txidHex
                            }.onFailure { payErr = it.message }
                            busy = false
                        }.start()
                    },
                ) { Text(if (busy) "Sending…" else "Confirm and send") }
            } else {
                TextButton(onClick = onDone) { Text("Done") }
            }
        },
        dismissButton = {
            if (sentTx == null) {
                TextButton(enabled = !busy, onClick = onDone) { Text("Cancel") }
            }
        },
    )
}
