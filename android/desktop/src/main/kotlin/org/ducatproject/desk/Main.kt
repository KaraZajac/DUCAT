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
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.StoredMessage
import java.io.File
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus

/**
 * DUCAT Desk v2 — the phone's protocol brain with a desk's face.
 *
 * Everything below the UI is the *same compiled source* the Android app
 * ships: Mailbox's patience windows and atomic sends, ContactStore's
 * partitioned prekeys, the chain rules. The desk adds a window: contacts on
 * the left, the thread on the right, a card on screen for a phone to claim.
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

fun main() {
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

        // The node, then the poller: the same loop the phone's service runs.
        LaunchedEffect(Unit) {
            withContext(Dispatchers.IO) {
                runCatching { nodeStart(File(deskDir, "veilid").absolutePath) }
                    .onFailure { error = it.message }
            }
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
                    }
                    val store = ContactStore(context)
                    contacts = store.all().sortedBy { it.displayName().lowercase() }
                    selected?.let { thread = store.thread(it) }
                }
                delay(4_000)
            }
        }

        MaterialTheme(colorScheme = darkColorScheme()) {
            Surface(Modifier.fillMaxSize()) {
                Column(Modifier.fillMaxSize()) {
                    // Top bar: who this desk is, and how connected.
                    Row(
                        Modifier.fillMaxWidth().padding(12.dp),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text("DUCAT Desk", style = MaterialTheme.typography.titleLarge)
                        Text(statusLine, style = MaterialTheme.typography.bodySmall)
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
                            LaunchedEffect(thread.size) {
                                if (thread.isNotEmpty()) listState.scrollToItem(thread.size - 1)
                            }
                            LazyColumn(Modifier.weight(1f), state = listState) {
                                items(thread) { m ->
                                    val who = if (m.outgoing) "me" else "them"
                                    val extra = when (m.kind) {
                                        1 -> "  [asks for ${m.amountPxmr} pXMR]"
                                        2 -> "  [sent ${m.amountPxmr} pXMR]"
                                        3 -> "  [receipt for ${m.amountPxmr} pXMR]"
                                        5 -> "  [withdrew message ${m.reSeq}]"
                                        6 -> "  [offers to drive — ${m.amountPxmr} pXMR]"
                                        7 -> "  [ride accepted — ${m.amountPxmr} pXMR]"
                                        else -> ""
                                    }
                                    Text(
                                        "$who: ${m.body}$extra",
                                        style = MaterialTheme.typography.bodyMedium,
                                        modifier = Modifier.padding(vertical = 2.dp),
                                    )
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
            }
        }
    }
}
