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
import androidx.compose.material.icons.filled.ArrowUpward
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
import androidx.compose.material.icons.filled.Receipt
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import org.ducatproject.ducat.formatXmr

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
                colors = TopAppBarDefaults.topAppBarColors(
                containerColor = MaterialTheme.colorScheme.background,
            ),
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
            Surface(color = MaterialTheme.colorScheme.background) {
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
                            // The cat, because it is the app's money button
                            // everywhere now — the same mark as the bar's
                            // centre. An Image: tinting it makes it a blob.
                            androidx.compose.foundation.Image(
                                androidx.compose.ui.res.painterResource(
                                    org.ducatproject.ducat.R.drawable.ducat_cat
                                ),
                                contentDescription = "Send or ask for money",
                                modifier = Modifier.size(30.dp),
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
        // The contact rides along, not just the address. Paying a request as a
        // bare address silently dropped the payment notice — the vendor never
        // learned which transaction answered their bill, and nothing could be
        // marked paid. The request's own payto is already on the contact:
        // receiving a request stores it as their freshest address (§16.12).
        PaySheet(
            prefillContact = c,
            prefillAmountPxmr = r.amountPxmr,
        ) { payRequest = null }
    }

    // The same sheet the send/request button opens, with the contact already
    // chosen. A second, smaller money form in chat meant one place had a
    // currency switch and a number pad and the other did not, which is how the
    // unit someone is thinking in stops matching the unit they are typing in.
    if (askOpen) {
        PaySheet(prefillContact = c) { askOpen = false }
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
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(
                            when (m.kind) {
                                1 -> Icons.Filled.RequestQuote
                                3 -> Icons.Filled.Receipt
                                else -> Icons.Filled.ArrowUpward
                            },
                            null,
                            Modifier.size(14.dp),
                            tint = fg.copy(alpha = 0.8f),
                        )
                        Spacer(Modifier.width(4.dp))
                        Text(
                            when {
                                m.kind == 1 && m.outgoing -> "You asked for"
                                m.kind == 1 -> "Asked you for"
                                // A receipt is issued by whoever *received* the
                                // money, so the direction reads the other way
                                // round from a notice.
                                m.kind == 3 && m.outgoing -> "Receipt you issued"
                                m.kind == 3 -> "Receipt"
                                m.outgoing -> "You sent"
                                else -> "Sent you"
                            },
                            style = MaterialTheme.typography.labelSmall,
                            color = fg.copy(alpha = 0.8f),
                        )
                    }
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
                    if (m.items.isNotEmpty()) {
                        Spacer(Modifier.height(8.dp))
                        Bill(m, fg)
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

/**
 * The breakdown, as a bill reads on paper.
 *
 * Monospace and right-aligned amounts, because a column of numbers that does
 * not line up is a column nobody checks. And checking is the point: core
 * refuses a message whose items and tax do not equal its amount (§16.13), so
 * what is drawn here always adds up — the reader can confirm it by eye, which
 * is a different and better thing from being told to trust it.
 *
 * The network fee is deliberately absent. A Monero fee is paid by the sender to
 * the network, not by the payer to the vendor, so a fee line on a vendor's bill
 * charges it twice: once in the total asked for and again when the payer's own
 * wallet builds the transaction. What the transfer cost is on the payer's
 * Activity screen, from their own record, which is the only place it can be
 * stated truthfully.
 */
@Composable
private fun Bill(m: StoredMessage, fg: androidx.compose.ui.graphics.Color) {
    val context = LocalContext.current
    val subtotal = m.items.sumOf { it.amountPxmr }

    @Composable
    fun line(label: String, pxmr: Long, strong: Boolean = false) {
        Row(Modifier.fillMaxWidth()) {
            Text(
                label,
                style = MaterialTheme.typography.labelSmall,
                color = fg.copy(alpha = if (strong) 1f else 0.85f),
                fontWeight = if (strong) FontWeight.SemiBold else FontWeight.Normal,
                modifier = Modifier.weight(1f),
            )
            Spacer(Modifier.width(8.dp))
            Text(
                formatXmr(pxmr),
                style = MaterialTheme.typography.labelSmall,
                fontFamily = FontFamily.Monospace,
                color = fg.copy(alpha = if (strong) 1f else 0.85f),
                fontWeight = if (strong) FontWeight.SemiBold else FontWeight.Normal,
            )
        }
    }

    Column(
        Modifier
            .background(fg.copy(alpha = 0.08f), RoundedCornerShape(8.dp))
            .padding(10.dp)
    ) {
        m.items.forEach { line(it.description, it.amountPxmr) }
        if (m.taxPxmr != null) {
            Spacer(Modifier.height(4.dp))
            HorizontalDivider(color = fg.copy(alpha = 0.25f))
            Spacer(Modifier.height(4.dp))
            line("subtotal", subtotal)
            line("tax", m.taxPxmr)
        }
        Spacer(Modifier.height(4.dp))
        HorizontalDivider(color = fg.copy(alpha = 0.25f))
        Spacer(Modifier.height(4.dp))
        line("total", m.amountPxmr, strong = true)
        Amounts.show(context, m.amountPxmr).secondary?.let {
            Text(
                it,
                style = MaterialTheme.typography.labelSmall,
                color = fg.copy(alpha = 0.7f),
                modifier = Modifier.align(Alignment.End),
            )
        }
    }
}
