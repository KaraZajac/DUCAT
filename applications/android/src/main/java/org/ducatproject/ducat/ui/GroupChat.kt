package org.ducatproject.ducat.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Groups
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.R
import org.ducatproject.ducat.StoredMessage

/**
 * A group's conversation (§16.19): the merge of its fan-out copies across the
 * pairwise threads, one row per (sender, group counter).
 *
 * The screen owns the two disclosures the section requires. The standing one
 * is the mesh gate — the composer refuses while this phone is missing a
 * member, and says *who* rather than dimming a button, because "add Dave" is
 * an instruction and a grey button is a shrug. The one-time one is the plain
 * statement of the shape: trusted people, add-only, no history for newcomers,
 * unforgeable member-to-member, leaving is local. Shown once per group per
 * phone, at creation or on first open after being added.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun GroupChatScreen(idHex: String, onBack: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val version by ContactStore.changes.collectAsState()
    val group = remember(version, idHex) { Groups.get(context, idHex) } ?: run {
        onBack(); return
    }
    val rows = remember(version, idHex) { Groups.thread(context, idHex) }
    val missing = remember(version, idHex) { Groups.missing(context, idHex) }
    val mine = remember { PersonaStore(context).personaHex() }
    val contacts = remember(version) { ContactStore(context).all() }
    val youLabel = stringResource(R.string.group_you)
    fun nameOf(hex: String): String = when {
        hex == mine -> youLabel
        else -> contacts.firstOrNull { it.personaHex == hex }?.displayName()
            ?: "${hex.take(8)}…"
    }

    var draft by rememberSaveable { mutableStateOf("") }
    var sending by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    // (sender, groupSeq) of the message being answered, if one is.
    var replyTo by rememberSaveable { mutableStateOf<String?>(null) }
    var addOpen by remember { mutableStateOf(false) }

    // The one-time disclosure, before anything else on a fresh group.
    if (!group.disclosed) {
        AlertDialog(
            onDismissRequest = {},
            title = { Text(stringResource(R.string.group_disclosure_title)) },
            text = {
                Column {
                    Text(stringResource(R.string.group_disclosure_body))
                }
            },
            confirmButton = {
                TextButton(onClick = { Groups.markDisclosed(context, idHex) }) {
                    Text(stringResource(R.string.group_disclosure_ok))
                }
            },
        )
    }

    val listState = rememberLazyListState()
    LaunchedEffect(rows.size) {
        if (rows.isNotEmpty()) listState.animateScrollToItem(rows.size - 1)
    }

    Column(Modifier.fillMaxSize()) {
        TopAppBar(
            colors = TopAppBarDefaults.topAppBarColors(
                containerColor = MaterialTheme.colorScheme.background,
            ),
            title = {
                Column {
                    Text(isolate(group.name), maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(
                        pluralStringResource(
                            R.plurals.group_members, group.members.size, group.members.size,
                        ),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            },
            navigationIcon = {
                IconButton(onClick = onBack) {
                    Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.chat_back))
                }
            },
            actions = {
                TextButton(onClick = { addOpen = true }) {
                    Text(stringResource(R.string.group_add))
                }
            },
        )

        LazyColumn(Modifier.weight(1f), state = listState) {
            items(rows, key = { r -> "${r.senderHex}:${r.message.groupSeq}" }) { r ->
                val m = r.message
                GroupBubble(
                    m = m,
                    own = m.outgoing,
                    senderName = nameOf(r.senderHex),
                    // The quote is the reader's own copy of the target,
                    // resolved by (sender, counter) — never bytes from the
                    // wire, so an unsend stays an unsend here too.
                    answering = m.groupReSender?.let { rs ->
                        m.groupReSeq?.let { rq ->
                            rows.firstOrNull { it.senderHex == rs && it.message.groupSeq == rq }
                                ?.message?.body?.takeIf { b -> b.isNotBlank() }
                                ?: stringResource(R.string.chat_reply_to_gone)
                        }
                    },
                    onReply = { replyTo = "${r.senderHex}:${m.groupSeq}" },
                )
            }
        }

        // The mesh gate: who is missing, by name, and nothing to press until
        // the person is a contact. Receiving stays open — this bars the door
        // outward only, which is the direction that can silently fail.
        if (missing.isNotEmpty()) {
            Surface(color = MaterialTheme.colorScheme.errorContainer) {
                Text(
                    stringResource(
                        R.string.group_mesh_incomplete,
                        missing.joinToString(", ") { "${it.take(8)}…" },
                    ),
                    Modifier.fillMaxWidth().padding(12.dp),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onErrorContainer,
                )
            }
        }
        error?.let {
            Text(
                it, Modifier.padding(horizontal = 16.dp),
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall,
            )
        }
        replyTo?.let { target ->
            Row(
                Modifier.fillMaxWidth().padding(start = 12.dp, end = 12.dp, top = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(
                    Modifier.width(3.dp).height(24.dp)
                        .background(MaterialTheme.colorScheme.primary, RoundedCornerShape(2.dp)),
                )
                Spacer(Modifier.width(8.dp))
                val (rs, rq) = target.split(":").let { it[0] to it[1].toLong() }
                Text(
                    rows.firstOrNull { it.senderHex == rs && it.message.groupSeq == rq }
                        ?.message?.body ?: stringResource(R.string.chat_reply_to_gone),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1, overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                IconButton(onClick = { replyTo = null }) {
                    Icon(Icons.Filled.Close, stringResource(R.string.chat_reply_cancel), Modifier.size(18.dp))
                }
            }
        }
        Row(
            Modifier.padding(12.dp).fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OutlinedTextField(
                value = draft,
                onValueChange = { if (it.length <= 2000) draft = it },
                placeholder = { Text(stringResource(R.string.chat_message_placeholder)) },
                modifier = Modifier.weight(1f),
                enabled = missing.isEmpty(),
                maxLines = 4,
            )
            Spacer(Modifier.width(8.dp))
            IconButton(
                onClick = {
                    val body = draft.trim()
                    if (body.isEmpty() || sending) return@IconButton
                    sending = true; error = null
                    val target = replyTo?.split(":")
                    scope.launch {
                        val r = withContext(Dispatchers.IO) {
                            runCatching {
                                Groups.send(
                                    context, idHex, body,
                                    reSender = target?.get(0),
                                    reSeq = target?.get(1)?.toLong(),
                                )
                            }
                        }
                        sending = false
                        r.onSuccess { all ->
                            draft = ""; replyTo = null
                            if (!all) error = context.getString(R.string.group_partial_queued)
                        }.onFailure { error = moneyFailure(context, it) }
                    }
                },
                enabled = missing.isEmpty() && draft.isNotBlank() && !sending,
            ) {
                if (sending) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                else Icon(Icons.AutoMirrored.Filled.Send, stringResource(R.string.chat_send))
            }
        }
    }

    if (addOpen) {
        val candidates = contacts.filter { it.personaHex !in group.members }
        AlertDialog(
            onDismissRequest = { addOpen = false },
            title = { Text(stringResource(R.string.group_add_title)) },
            text = {
                Column {
                    Text(
                        stringResource(R.string.group_add_note),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(Modifier.height(8.dp))
                    if (candidates.isEmpty()) {
                        Text(stringResource(R.string.group_add_nobody))
                    }
                    candidates.forEach { c ->
                        Text(
                            c.displayName(),
                            Modifier.fillMaxWidth()
                                .clickable {
                                    addOpen = false
                                    scope.launch(Dispatchers.IO) {
                                        runCatching { Groups.add(context, idHex, c.personaHex) }
                                    }
                                }
                                .padding(vertical = 10.dp),
                        )
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { addOpen = false }) {
                    Text(stringResource(R.string.chat_cancel))
                }
            },
        )
    }
}

/** One group message: the sender's name over the words, a reply quote when
 *  one is named, long-press to answer it. */
@Composable
private fun GroupBubble(
    m: StoredMessage,
    own: Boolean,
    senderName: String,
    answering: String?,
    onReply: () -> Unit,
) {
    val align = if (own) Alignment.End else Alignment.Start
    val bg = if (own) MaterialTheme.colorScheme.primaryContainer
    else MaterialTheme.colorScheme.surfaceVariant
    val fg = if (own) MaterialTheme.colorScheme.onPrimaryContainer
    else MaterialTheme.colorScheme.onSurfaceVariant
    Column(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 3.dp),
        horizontalAlignment = align,
    ) {
        Surface(
            color = bg,
            shape = RoundedCornerShape(14.dp),
            modifier = Modifier.widthIn(max = 300.dp)
                .clickable(onClick = onReply),
        ) {
            Column(Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) {
                if (!own) {
                    Text(
                        isolate(senderName),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.primary,
                    )
                }
                answering?.let {
                    Row(Modifier.padding(vertical = 3.dp)) {
                        Box(
                            Modifier.width(3.dp).height(20.dp)
                                .background(fg.copy(alpha = 0.5f), RoundedCornerShape(2.dp)),
                        )
                        Spacer(Modifier.width(6.dp))
                        Text(
                            it,
                            style = MaterialTheme.typography.bodySmall,
                            color = fg.copy(alpha = 0.75f),
                            fontStyle = FontStyle.Italic,
                            maxLines = 1, overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
                Text(isolate(m.body), color = fg)
            }
        }
    }
}

/**
 * Making a group: a name, and members picked from *contacts only* — the list
 * is the constraint made visible, since fan-out can only ever reach people
 * this phone already holds. The disclosure fires on the group's first open.
 */
@Composable
fun GroupCreateScreen(onDone: (String) -> Unit, onCancel: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val contacts = remember { ContactStore(context).all() }
    var name by rememberSaveable { mutableStateOf("") }
    var picked by remember { mutableStateOf(setOf<String>()) }
    var busy by remember { mutableStateOf(false) }

    Column(Modifier.fillMaxSize().padding(16.dp)) {
        Text(stringResource(R.string.group_create_title), style = MaterialTheme.typography.titleLarge)
        Spacer(Modifier.height(4.dp))
        Text(
            stringResource(R.string.group_create_note),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(12.dp))
        OutlinedTextField(
            value = name,
            onValueChange = { if (it.length <= 48) name = it },
            label = { Text(stringResource(R.string.group_name_label)) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(12.dp))
        LazyColumn(Modifier.weight(1f)) {
            items(contacts, key = { it.personaHex }) { c ->
                Row(
                    Modifier.fillMaxWidth()
                        .clickable {
                            picked = if (c.personaHex in picked) picked - c.personaHex
                            else picked + c.personaHex
                        }
                        .padding(vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Checkbox(
                        checked = c.personaHex in picked,
                        onCheckedChange = null,
                    )
                    Spacer(Modifier.width(8.dp))
                    Text(c.displayName())
                }
            }
        }
        Row {
            OutlinedButton(onClick = onCancel, modifier = Modifier.weight(1f)) {
                Text(stringResource(R.string.chat_cancel))
            }
            Spacer(Modifier.width(12.dp))
            Button(
                onClick = {
                    busy = true
                    scope.launch {
                        val id = withContext(Dispatchers.IO) {
                            runCatching {
                                Groups.create(context, name.trim(), picked.toList())
                            }.getOrNull()
                        }
                        busy = false
                        id?.let { onDone(it.idHex) }
                    }
                },
                enabled = !busy && name.isNotBlank() && picked.size >= 2,
                modifier = Modifier.weight(1f),
            ) {
                if (busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                else Text(stringResource(R.string.group_create_button))
            }
        }
    }
}
