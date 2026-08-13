package org.ducatproject.ducat.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.LockOpen
import androidx.compose.material.icons.filled.Send
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.threadAad
import org.ducatproject.ducat.StoredMessage
import uniffi.ducat_mobile.nodeAppCall
import uniffi.ducat_mobile.sealMessage
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

private const val MSG_TEXT: Byte = 0x41
private const val MSG_PREKEYS: Byte = 0x42

/**
 * One conversation.
 *
 * Every outgoing message is sealed to one of their published prekeys (§16.11)
 * and chained to the one before it (§16.10). Neither is decoration: the chain
 * is what makes a *removed and replaced* message visible, and the prekey is
 * what makes a delivered message unrecoverable afterwards.
 *
 * The screen shows when a message went out **without** forward secrecy, because
 * §16.11 requires the fallback be visible rather than silently accepted.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatScreen(contact: Contact, onBack: () -> Unit) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val store = remember { ContactStore(context) }
    val scope = rememberCoroutineScope()
    var c by remember { mutableStateOf(contact) }
    val mine = remember { PersonaStore(context).personaHex() }
    var messages by remember { mutableStateOf(store.thread(contact.personaHex)) }
    var draft by remember { mutableStateOf("") }
    var sending by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val listState = rememberLazyListState()

    // Re-read whenever anything writes to the store. The responder runs in a
    // different coroutine and cannot reach this screen's state directly; without
    // this, an inbound message was decrypted and stored and then stayed
    // invisible until the user sent something of their own.
    val version by ContactStore.changes.collectAsState()
    LaunchedEffect(version) {
        messages = store.thread(c.personaHex)
        store.all().firstOrNull { it.personaHex == c.personaHex }?.let { c = it }
    }

    LaunchedEffect(messages.size) {
        if (messages.isNotEmpty()) listState.animateScrollToItem(messages.size)
    }
    // The keyboard opening does not change the message count, so the scroll
    // above never fired for it and a sent message ended up behind the keyboard
    // until the user dismissed it. Watching the IME inset is what actually
    // tracks "the visible area just shrank".
    val imeBottom = WindowInsets.ime.getBottom(LocalDensity.current)
    LaunchedEffect(imeBottom) {
        if (messages.isNotEmpty()) listState.animateScrollToItem(messages.size)
    }

    Scaffold(
        modifier = Modifier.imePadding(),
        topBar = {
            TopAppBar(
                title = { Text(c.displayName()) },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.Filled.ArrowBack, "Back") }
                },
            )
        },
        bottomBar = {
            Surface(tonalElevation = 3.dp) {
                Column {
                    error?.let {
                        Text(
                            it,
                            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp),
                            color = MaterialTheme.colorScheme.error,
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                    Row(
                        Modifier.padding(12.dp).fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        OutlinedTextField(
                            value = draft,
                            onValueChange = { if (it.length <= 2000) draft = it },
                            placeholder = { Text("Message") },
                            modifier = Modifier.weight(1f),
                            maxLines = 4,
                        )
                        Spacer(Modifier.width(8.dp))
                        FilledIconButton(
                            onClick = {
                                val body = draft.trim()
                                if (body.isEmpty()) return@FilledIconButton
                                sending = true
                                error = null
                                scope.launch {
                                    val result = withContext(Dispatchers.IO) {
                                        runCatching { sendOne(store, c, body, mine) }
                                    }
                                    sending = false
                                    result.onSuccess { updated ->
                                        c = updated
                                        draft = ""
                                        messages = store.thread(c.personaHex)
                                    }.onFailure {
                                        error = it.message ?: "could not send"
                                    }
                                }
                            },
                            enabled = !sending && draft.isNotBlank() && c.theirBundle != null,
                        ) {
                            if (sending) {
                                CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                            } else {
                                Icon(Icons.Filled.Send, "Send")
                            }
                        }
                    }
                    if (c.theirBundle == null) {
                        Text(
                            "Fetching their keys — they need to be online once before " +
                                "the first message can be encrypted.",
                            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
        },
    ) { padding ->
        LazyColumn(
            Modifier.padding(padding).fillMaxSize(),
            state = listState,
            contentPadding = PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            item {
                Text(
                    "Messages are encrypted to keys that are deleted after use. " +
                        "Once read, they cannot be recovered — not even by you.",
                    Modifier.fillMaxWidth().padding(bottom = 12.dp),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                )
            }
            items(messages) { m -> Bubble(m) }
        }
    }

    // Fetch their prekey bundle the first time the screen opens.
    LaunchedEffect(c.personaHex) {
        if (c.theirBundle == null) {
            withContext(Dispatchers.IO) {
                runCatching {
                    val reply = nodeAppCall(c.rendezvous, byteArrayOf(MSG_PREKEYS), 20_000u)
                    store.setTheirBundle(c.personaHex, reply)
                    store.all().first { it.personaHex == c.personaHex }
                }
            }.onSuccess { c = it }
                .onFailure { error = "Could not reach them: ${it.message}" }
        }
    }
}

/** Seal, chain, send, record. Fails as a unit — nothing is stored unsent. */
private fun sendOne(store: ContactStore, c: Contact, body: String, minePersonaHex: String): Contact {
    val bundle = c.theirBundle ?: throw IllegalStateException("no keys for this contact yet")
    val sealed = sealMessage(
        bundle,
        c.outSeq.toULong(),
        c.outPrevLink ?: ByteArray(32),
        body,
        // Binds the ciphertext to this pair, so it cannot be replayed into
        // another conversation (§16.11). Symmetric — see threadAad.
        threadAad(minePersonaHex, c.personaHex),
    )
    val framed = byteArrayOf(MSG_TEXT) + sealed.bytes
    nodeAppCall(c.rendezvous, framed, 30_000u)

    store.append(
        c.personaHex,
        StoredMessage(
            outgoing = true,
            seq = c.outSeq,
            body = body,
            timestamp = System.currentTimeMillis() / 1000,
            forwardSecret = sealed.forwardSecret,
        ),
    )
    store.advanceOutbound(c.personaHex, c.outSeq + 1, sealed.nextLink)
    // Re-read rather than returning a locally-mutated copy: the responder may
    // have advanced the inbound counters while this send was in flight, and the
    // screen must not carry a view of the contact that predates them.
    return store.all().first { it.personaHex == c.personaHex }
}

@Composable
private fun Bubble(m: StoredMessage) {
    val align = if (m.outgoing) Alignment.End else Alignment.Start
    val bg = if (m.outgoing) MaterialTheme.colorScheme.primaryContainer
    else MaterialTheme.colorScheme.surfaceVariant
    val fg = if (m.outgoing) MaterialTheme.colorScheme.onPrimaryContainer
    else MaterialTheme.colorScheme.onSurfaceVariant

    Column(Modifier.fillMaxWidth(), horizontalAlignment = align) {
        Box(
            Modifier
                .widthIn(max = 280.dp)
                .background(bg, RoundedCornerShape(16.dp))
                .padding(horizontal = 14.dp, vertical = 10.dp)
        ) {
            Text(m.body, color = fg)
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (!m.forwardSecret) {
                // §16.11: the signed-prekey fallback is a real weakening and is
                // surfaced rather than swallowed.
                Icon(
                    Icons.Filled.LockOpen,
                    "Sent without forward secrecy",
                    Modifier.size(12.dp),
                    tint = MaterialTheme.colorScheme.error,
                )
                Spacer(Modifier.width(4.dp))
            }
            Text(
                SimpleDateFormat("HH:mm", Locale.getDefault())
                    .format(Date(m.timestamp * 1000)),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.outline,
            )
        }
    }
}
