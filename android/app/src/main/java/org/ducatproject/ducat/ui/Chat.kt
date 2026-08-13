package org.ducatproject.ducat.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.RequestQuote
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
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.WalletStore
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.threadAad
import org.ducatproject.ducat.StoredMessage
import org.ducatproject.ducat.Amounts
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

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

    var settingsOpen by remember { mutableStateOf(false) }
    var askOpen by remember { mutableStateOf(false) }
    var payRequest by remember { mutableStateOf<StoredMessage?>(null) }
    var confirmDelete by remember { mutableStateOf<StoredMessage?>(null) }

    // Applied on open and whenever the thread changes, because nothing else
    // runs while a conversation sits idle.
    LaunchedEffect(version) {
        val secs = store.disappearAfter(c.personaHex)
        if (secs > 0 && store.expireOld(c.personaHex, secs) > 0) {
            messages = store.thread(c.personaHex)
        }
    }

    Scaffold(
        modifier = Modifier.imePadding(),
        topBar = {
            TopAppBar(
                title = { Text(c.displayName()) },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.Filled.ArrowBack, "Back") }
                },
                actions = {
                    IconButton(onClick = { settingsOpen = true }) {
                        Icon(Icons.Filled.MoreVert, "Conversation settings")
                    }
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
                        IconButton(onClick = { askOpen = true }, enabled = c.theirBundle != null) {
                            Icon(
                                Icons.Filled.RequestQuote,
                                "Ask for money",
                                tint = MaterialTheme.colorScheme.tertiary,
                            )
                        }
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
                                        runCatching { sendOne(context, c, body, mine) }
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
                            "No keys for this contact — the handshake did not complete. " +
                                "Ask them for a new card.",
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
            items(messages) { m ->
                Bubble(m, onLongPress = { confirmDelete = m }, onPay = { payRequest = it })
            }
        }
    }

    // Nothing to fetch: their prekeys arrived with the handshake and live in
    // the contact record. §16.12's whole point is that the first message needs
    // no round trip to someone who may not be there.

    payRequest?.let { r ->
        PaySheet(
            prefillAddress = r.payto,
            prefillAmountPxmr = r.amountPxmr,
        ) { payRequest = null }
    }

    if (askOpen) {
        AskForMoneyDialog(
            onDismiss = { askOpen = false },
            onSend = { pxmr, note ->
                askOpen = false
                sending = true
                error = null
                scope.launch {
                    val r = withContext(Dispatchers.IO) {
                        runCatching {
                            Mailbox.send(
                                context, c, note, mine,
                                kind = 1,
                                amountPxmr = pxmr,
                                // Our address travels with the ask, so they need
                                // nothing from a record that may be stale (§16.13).
                                payto = WalletStore(context).address(),
                            )
                        }
                    }
                    sending = false
                    r.onSuccess { c = it; messages = store.thread(c.personaHex) }
                        .onFailure { error = it.message ?: "could not send" }
                }
            },
        )
    }

    if (settingsOpen) {
        ChatSettingsDialog(
            current = store.disappearAfter(c.personaHex),
            onPick = { store.setDisappearAfter(c.personaHex, it); settingsOpen = false },
            onClearAll = {
                store.deleteThread(c.personaHex)
                messages = emptyList()
                settingsOpen = false
            },
            onDismiss = { settingsOpen = false },
        )
    }

    confirmDelete?.let { m ->
        AlertDialog(
            onDismissRequest = { confirmDelete = null },
            title = { Text("Delete this message?") },
            text = {
                Text(
                    "Removed from this phone only. The other side keeps their " +
                        "copy — nothing here can reach it."
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    store.deleteMessage(c.personaHex, m.seq, m.outgoing)
                    messages = store.thread(c.personaHex)
                    confirmDelete = null
                }) { Text("Delete", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = { confirmDelete = null }) { Text("Cancel") } },
        )
    }
}

/**
 * Disappearing messages, stated honestly.
 *
 * This is a **local** retention rule. It cannot reach the other device, and a
 * screen implying otherwise would be worse than not offering it — the whole
 * point of §16.11 is not overstating what a guarantee covers.
 */
@Composable
private fun ChatSettingsDialog(
    current: Long,
    onPick: (Long) -> Unit,
    onClearAll: () -> Unit,
    onDismiss: () -> Unit,
) {
    val options = listOf(
        0L to "Keep everything",
        3600L to "1 hour",
        86_400L to "1 day",
        604_800L to "1 week",
    )
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Conversation") },
        text = {
            Column {
                Text("Delete messages on this phone after", style = MaterialTheme.typography.labelLarge)
                Spacer(Modifier.height(8.dp))
                options.forEach { (secs, label) ->
                    Row(
                        Modifier.fillMaxWidth().clickable { onPick(secs) }.padding(vertical = 6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(selected = current == secs, onClick = { onPick(secs) })
                        Spacer(Modifier.width(8.dp))
                        Text(label)
                    }
                }
                Spacer(Modifier.height(10.dp))
                Text(
                    "Only your copy. The other side keeps theirs, and nothing here " +
                        "can reach it.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        },
        confirmButton = {
            TextButton(onClick = onClearAll) {
                Text("Clear this chat", color = MaterialTheme.colorScheme.error)
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Done") } },
    )
}

/** Seal, chain, append. Fails as a unit — nothing is stored unsent. */
private fun sendOne(
    context: android.content.Context,
    c: Contact,
    body: String,
    minePersonaHex: String,
): Contact = Mailbox.send(context, c, body, minePersonaHex)

/**
 * Asking for money.
 *
 * The amount is entered in XMR and carried in piconero, because a rounded
 * amount in a message someone acts on is a rounding error somebody pays for.
 */
@Composable
private fun AskForMoneyDialog(onDismiss: () -> Unit, onSend: (Long, String) -> Unit) {
    var amount by remember { mutableStateOf("") }
    var note by remember { mutableStateOf("") }
    val pxmr = remember(amount) {
        amount.trim().toBigDecimalOrNull()
            ?.multiply(java.math.BigDecimal(1_000_000_000_000L))
            ?.toLong()
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Ask for money") },
        text = {
            Column {
                OutlinedTextField(
                    value = amount,
                    onValueChange = { amount = it },
                    label = { Text("Amount (XMR)") },
                    singleLine = true,
                    isError = amount.isNotBlank() && pxmr == null,
                )
                Spacer(Modifier.height(10.dp))
                OutlinedTextField(
                    value = note,
                    onValueChange = { if (it.length <= 128) note = it },
                    label = { Text("What for") },
                    singleLine = true,
                )
                Spacer(Modifier.height(12.dp))
                Text(
                    "This is a message, not a charge. They still have to choose to " +
                        "pay it, and nothing here can take anything.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = { pxmr?.let { onSend(it, note.ifBlank { "Payment request" }) } },
                enabled = pxmr != null && pxmr > 0,
            ) { Text("Ask") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun Bubble(m: StoredMessage, onLongPress: () -> Unit, onPay: (StoredMessage) -> Unit) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val align = if (m.outgoing) Alignment.End else Alignment.Start
    // Yours in the accent, theirs in the neutral surface. Colour is doing the
    // work here rather than alignment alone, because alignment is unreadable on
    // a narrow screen once messages wrap to full width.
    val bg = if (m.outgoing) MaterialTheme.colorScheme.primary
    else MaterialTheme.colorScheme.surfaceVariant
    val fg = if (m.outgoing) MaterialTheme.colorScheme.onPrimary
    else MaterialTheme.colorScheme.onSurfaceVariant
    val corner = if (m.outgoing) {
        RoundedCornerShape(16.dp, 16.dp, 4.dp, 16.dp)
    } else {
        RoundedCornerShape(16.dp, 16.dp, 16.dp, 4.dp)
    }

    Column(Modifier.fillMaxWidth(), horizontalAlignment = align) {
        Box(
            Modifier
                .widthIn(max = 280.dp)
                .background(bg, corner)
                .combinedClickable(onClick = {}, onLongClick = onLongPress)
                .padding(horizontal = 14.dp, vertical = 10.dp)
        ) {
            if (m.kind == 0) {
                Text(m.body, color = fg)
            } else {
                Column {
                    Text(
                        if (m.kind == 1) "Asked for" else "Sent",
                        style = MaterialTheme.typography.labelSmall,
                        color = fg.copy(alpha = 0.8f),
                    )
                    val a = Amounts.show(context, m.amountPxmr)
                    Text(
                        a.primary,
                        style = MaterialTheme.typography.titleMedium,
                        color = fg,
                    )
                    a.secondary?.let {
                        Text(it, style = MaterialTheme.typography.labelSmall,
                             color = fg.copy(alpha = 0.75f))
                    }
                    if (m.body.isNotBlank()) {
                        Spacer(Modifier.height(2.dp))
                        Text(m.body, color = fg, style = MaterialTheme.typography.bodySmall)
                    }
                    // §16.13: a request carries no authority. An incoming one
                    // that offered a one-tap "pay" would be exactly the shortcut
                    // §15.5's confirm screen exists to prevent.
                    if (m.kind == 1 && !m.outgoing) {
                        Spacer(Modifier.height(8.dp))
                        if (m.payto != null) {
                            // Opens the send screen filled in. It does **not**
                            // pay: §16.13 forbids a one-tap spend from an
                            // arriving message, because the confirm screen is
                            // the only thing between a message and money
                            // leaving.
                            FilledTonalButton(
                                onClick = { onPay(m) },
                                contentPadding = PaddingValues(horizontal = 12.dp, vertical = 4.dp),
                            ) { Text("Review payment", style = MaterialTheme.typography.labelMedium) }
                        } else {
                            Text(
                                "No address in this request — ask them where to send it.",
                                style = MaterialTheme.typography.labelSmall,
                                color = fg.copy(alpha = 0.7f),
                            )
                        }
                    }
                }
            }
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
