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
import org.ducatproject.ducat.Groups
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.NameStore
import org.ducatproject.ducat.NodeStore
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.Rates
import org.ducatproject.ducat.Recurring
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
            val img = javax.imageio.ImageIO.read(
                DeskTray::class.java.getResourceAsStream("/desk-tray.png"),
            )
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
    // The phone's screens read their words through R; point the table at the
    // chosen language before any of them draws. LocaleStore is the phone's
    // own setting, so a desk and a phone in one household agree.
    val dir = dataDir()
    runCatching {
        android.res.DeskRes.setLocale(org.ducatproject.ducat.LocaleStore(DeskContext(dir)).tag())
    }
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
    Window(
        onCloseRequest = ::exitApplication,
        title = "DUCAT Desk",
        // The window's own icon: what a taskbar, an alt-tab and a Dock show.
        icon = androidx.compose.ui.res.painterResource("desk-icon.png"),
    ) {
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
        var error by remember { mutableStateOf<String?>(null) }
        var balances by remember { mutableStateOf<Balances?>(null) }
        var fiat by remember { mutableStateOf<String?>(null) }
        var deskName by remember { mutableStateOf<String?>(null) }
        var renameOpen by remember { mutableStateOf(false) }
        var receiveOpen by remember { mutableStateOf(false) }
        var room by remember { mutableStateOf(Room.Conversations) }
        // §4.3: the same gate the phone keeps. Read once at launch; the
        // first-run screen sets it when a backup has actually been exported.
        // The vault first: a locked desk cannot read the preference that
        // says whether it is onboarded, let alone a wallet.
        var locked by remember { mutableStateOf(!Unlock.tryQuiet(deskDir)) }
        var protected by remember {
            mutableStateOf(org.ducatproject.ducat.DeskVault.exists(deskDir))
        }
        var onboarded by remember(locked) {
            mutableStateOf(
                !locked && org.ducatproject.ducat.ui.ThemePreference(context).onboarded,
            )
        }
        var payOpen by remember { mutableStateOf(false) }
        var profileFor by remember { mutableStateOf<Contact?>(null) }
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
        LaunchedEffect(locked) {
            if (locked) return@LaunchedEffect
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
                        // The desk shares the scheduling screens, so it must
                        // share the firing too — a schedule the till can
                        // create but never send is a rent reminder that
                        // works only where nobody runs a till.
                        runCatching { Groups.retryOutbox(context) }
                        runCatching { Recurring.runDue(context) }
                        runCatching { Mailbox.verifyLastWrites(context) }
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

        // Every phone screen hosted here asks for LocalContext.current; the
        // desk's Context is the same one its stores already use.
        androidx.compose.runtime.CompositionLocalProvider(
            androidx.compose.ui.platform.LocalContext provides context,
        ) {
        MaterialTheme(colorScheme = darkColorScheme()) {
            Surface(Modifier.fillMaxSize()) {
                if (locked) {
                    UnlockScreen(deskDir, onUnlocked = { locked = false })
                    return@Surface
                }
                if (!protected) {
                    // Offered once, before anything is worth stealing.
                    ProtectStep(deskDir, onSettled = { protected = true })
                    return@Surface
                }
                if (!onboarded) {
                    // Nothing else is reachable until the keys have a copy
                    // that outlives this machine.
                    FirstRun(onDone = {
                        org.ducatproject.ducat.ui.ThemePreference(context).onboarded = true
                        onboarded = true
                    })
                    return@Surface
                }
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
                                onClick = { payOpen = true },
                            ) { Text("Send") }
                            Spacer(Modifier.width(8.dp))
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
                        RoomRail(room, unread.size) { room = it }
                        VerticalDivider()
                        if (room != Room.Conversations) {
                            // A phone screen, hosted whole.
                            when (room) {
                                Room.Till -> TillRoom()
                                Room.BarTab -> BarTabRoom()
                                Room.Donate -> DonateRoom()
                                Room.Activity -> ActivityRoom()
                                Room.Wallet -> WalletRoom(onTopUp = { receiveOpen = true })
                                Room.Ride -> RideRoom()
                                Room.Me -> MeRoom()
                                Room.Codes -> CodesRoom(
                                    onOpenChat = {
                                        selected = it.personaHex
                                        room = Room.Conversations
                                    },
                                    onScanAddress = { _, _ -> payOpen = true },
                                )
                                Room.Settings -> SettingsRoom()
                                else -> Unit
                            }
                            return@Row
                        }
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

                        // The thread: the phone's own ChatScreen, which is
                        // where all of it lives — bills that render as bills,
                        // receipts, reactions, attachments, the ride and
                        // escrow banners, the settlement counters. The desk
                        // used to draw a smaller imitation of this; it does
                        // not any more, because two chat screens is two
                        // places for a payment rule to be wrong.
                        Box(Modifier.weight(1f).fillMaxHeight()) {
                            val open = contacts.firstOrNull { it.personaHex == selected }
                            if (open == null) {
                                Box(
                                    Modifier.fillMaxSize(),
                                    contentAlignment = Alignment.Center,
                                ) {
                                    Text(
                                        "Pick someone on the left, or hand out a card.",
                                        style = MaterialTheme.typography.bodyMedium,
                                    )
                                }
                            } else {
                                org.ducatproject.ducat.ui.ChatScreen(
                                    contact = open,
                                    onBack = { selected = null },
                                )
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

                // The phone's own send screen: fiat and XMR, a fee-aware
                // Max, speed, memo, and the confirm that never one-taps —
                // its source, not a desk imitation of it.
                if (payOpen) {
                    org.ducatproject.ducat.ui.PaySheet(
                        prefillContact = contacts.firstOrNull { it.personaHex == selected },
                        onDismiss = { payOpen = false },
                    )
                }

                profileFor?.let { c ->
                    ProfileDialog(
                        contact = c,
                        onOpenChat = {
                            selected = it.personaHex
                            room = Room.Conversations
                            profileFor = null
                        },
                        onClose = { profileFor = null },
                    )
                }
            }
        }
        }
    }
}
