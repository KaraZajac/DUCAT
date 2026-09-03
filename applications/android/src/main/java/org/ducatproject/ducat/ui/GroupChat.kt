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
import androidx.compose.material.icons.automirrored.filled.CallSplit
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
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Groups
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.R
import org.ducatproject.ducat.StoredMessage

/** A list of hexes through a Bundle: the split sheet's picks and its billed set. */
private val HEX_LIST_SAVER = androidx.compose.runtime.saveable.listSaver<
    androidx.compose.runtime.snapshots.SnapshotStateList<String>, String,
>(
    save = { it.toList() },
    restore = { mutableStateListOf<String>().apply { addAll(it) } },
)

/**
 * A split that billed some of the table and not the rest. [fails] are the
 * names for the sheet's line; [billed] hold their bill already, and the
 * retry must skip them — the same bill twice is a duplicate, not a retry.
 */
private class SplitPartial(val fails: List<String>, val billed: List<String>) :
    Exception("split: ${fails.size} not reached")

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
    val store = remember { ContactStore(context) }
    val version by ContactStore.changes.collectAsState()
    val group = remember(version, idHex) { Groups.get(context, idHex) } ?: run {
        onBack(); return
    }
    // The merge reads one pairwise thread per member — work that scales
    // with the group and its history, so it happens off the main thread
    // (the ledger ANR's lesson). Empty for a beat on first open. The
    // contact book and the mesh check ride along: both are a decrypt and
    // a parse of the whole book, and they ran in composition on every
    // store bump — every message landing in any thread.
    var rows by remember(idHex) { mutableStateOf<List<Groups.Row>>(emptyList()) }
    var contacts by remember { mutableStateOf<List<org.ducatproject.ducat.Contact>>(emptyList()) }
    var missing by remember(idHex) { mutableStateOf<List<String>>(emptyList()) }
    LaunchedEffect(version, idHex) {
        val (fresh, book, gaps) = withContext(Dispatchers.IO) {
            Triple(Groups.thread(context, idHex), store.all(), Groups.missing(context, idHex))
        }
        rows = fresh
        contacts = book
        missing = gaps
        // Looking at the group is what "seen" means, as for a thread: the
        // list's dot and the tab badge clear when the eyes arrive.
        withContext(Dispatchers.IO) {
            Groups.markSeen(context, idHex, Groups.lookAt(context, fresh))
        }
    }
    // Reactions and retracts decorate; only words are bubbles. Same split the
    // pairwise screen makes, with the group reference doing the naming.
    val shownRows = remember(rows) { rows.filter { it.message.kind !in setOf(4, 5) } }
    // Latest emoji per (target, reactor) — changing your mind works here too.
    val reactions = remember(rows) {
        rows.filter { it.message.kind == 4 }
            .mapNotNull { r ->
                val rs = r.message.groupReSender ?: return@mapNotNull null
                val rq = r.message.groupReSeq ?: return@mapNotNull null
                Triple(rs to rq, r.senderHex, r.message)
            }
            .groupBy({ it.first }) { it.second to it.third }
            .mapValues { (_, list) ->
                list.groupBy { it.first }
                    .mapNotNull { (_, per) -> per.maxByOrNull { it.second.timestamp }?.second?.body }
            }
    }
    // A withdrawal counts only from the message's own author: anyone can send
    // a kind-5 naming anything, and honouring a stranger's would let any
    // member blank any other's words on every screen but the author's.
    val unsent = remember(rows) {
        rows.filter { it.message.kind == 5 }
            .mapNotNull { r ->
                val rs = r.message.groupReSender ?: return@mapNotNull null
                val rq = r.message.groupReSeq ?: return@mapNotNull null
                if (rs == r.senderHex) rs to rq else null
            }.toSet()
    }
    val mine = remember { PersonaStore(context).allHexes() }
    val youLabel = stringResource(R.string.group_you)
    fun nameOf(hex: String): String = when {
        hex in mine -> youLabel
        else -> contacts.firstOrNull { it.personaHex == hex }?.displayName()
            ?: "${hex.take(8)}…"
    }

    // The group's sends live under ThreadSends like a thread's, keyed so
    // they can never be mistaken for a persona's. A group send is a
    // fan-out — one write per member — so it runs for as long as the
    // largest group takes, and a screen-scoped send was cancelled by a
    // rotation after the writes and before the composer was emptied: the
    // words came back under a live Send button, and a second tap minted a
    // second group counter for the same sentence.
    val key = "group:$idHex"
    // Kept the way a thread's is, so Back mid-sentence costs nothing.
    // Keyed on the group, like every neighbour here. Unkeyed, switching
    // groups carried the composer text into the new room and wrote it to
    // that room's draft on the way out.
    var draft by rememberSaveable(idHex) { mutableStateOf(store.draftOf(key)) }
    // Keyed, for the reason Chat.kt's copy explains: unkeyed, this held the
    // NEW room's draft by the time the OLD room's onDispose ran, and filed
    // it under the old room's key.
    val latest = remember(idHex) { mutableStateOf(draft) }.apply { value = draft }
    DisposableEffect(idHex) {
        val mineKey = key
        onDispose {
            val d = latest.value
            store.saveDraft(mineKey, if (ThreadSends.owns(mineKey, d)) "" else d)
        }
    }
    var sending by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    // (sender, groupSeq) of the message being answered, if one is.
    // Keyed for a sharper reason: a reply target is "(sender, groupSeq)",
    // and Groups.send numbers with a *per-group* counter, so seq 1 exists
    // in every group. Carried across a switch, the banner points at one
    // room's message and the send quotes a real but unrelated line in the
    // other — which every member of that group then receives.
    var replyTo by rememberSaveable(idHex) { mutableStateOf<String?>(null) }
    var addOpen by remember { mutableStateOf(false) }
    // The split sheet survives a rotation: its bills go out one member at
    // a time and it keeps the list of who has been billed (see SplitSheet),
    // which a sheet that closed with the turn of the phone threw away.
    var splitOpen by rememberSaveable { mutableStateOf(false) }

    val send: (String?, () -> Boolean) -> Unit = { what, block ->
        sending = true
        error = null
        ThreadSends.launch(store, key, what) {
            // Groups.send says whether every copy landed; the ones that
            // did not are queued and retried by the poller, so the
            // message is out either way — this is a line, not a failure.
            if (block()) null else context.getString(R.string.group_partial_queued)
        }
    }
    val tick by ThreadSends.ticks.collectAsState()
    LaunchedEffect(tick) {
        sending = ThreadSends.inFlight(key)
        for (o in ThreadSends.take(key)) when (o) {
            is ThreadSends.Outcome.Landed -> {
                // Exactly the words that went, as on the pairwise screen:
                // the box stays live during the fan-out.
                val d = draft.trimStart()
                if (o.body != null && d.startsWith(o.body)) {
                    draft = d.removePrefix(o.body).trimStart()
                    replyTo = null
                }
                error = o.result
            }
            is ThreadSends.Outcome.Failed -> {
                if (o.body != null && draft.isBlank()) draft = o.body
                error = moneyFailure(context, o.error, orElse = {
                    if (o.what != null) context.getString(R.string.chat_could_not_send_the, o.what)
                    else context.getString(R.string.chat_could_not_send)
                })
                org.ducatproject.ducat.DucatLog.w(
                    "GroupChat",
                    "${o.what ?: "send"}: ${o.error.javaClass.simpleName}: ${o.error.message}",
                )
            }
        }
    }

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
    // The bubbles, not the rows: a reaction or a withdrawal landing is a
    // decoration on something already on screen, not a reason to pull
    // the reader to the bottom — and the index has to be one the list has.
    LaunchedEffect(shownRows.size) {
        if (shownRows.isNotEmpty()) listState.animateScrollToItem(shownRows.size - 1)
    }
    // The keyboard opening changes no count, so the last bubble ended up
    // behind it until it was dismissed — the pairwise screen's lesson: the
    // IME inset is what tracks "the visible area just shrank".
    val imeBottom = WindowInsets.ime.getBottom(androidx.compose.ui.platform.LocalDensity.current)
    LaunchedEffect(imeBottom) {
        if (shownRows.isNotEmpty()) listState.animateScrollToItem(shownRows.size - 1)
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

        var menuFor by remember { mutableStateOf<Groups.Row?>(null) }
        LazyColumn(Modifier.weight(1f), state = listState) {
            items(shownRows, key = { r -> "${r.senderHex}:${r.message.groupSeq}" }) { r ->
                val m = r.message
                val gone = (r.senderHex to m.groupSeq) in unsent
                // A message arriving takes its place instead of teleporting
                // into it — five people's fan-out lands out of order by
                // nature, and the motion is what makes an insertion above
                // the newest line read as "arrived late", not "appeared".
                Box(Modifier.animateItem()) {
                GroupBubble(
                    m = m,
                    own = m.outgoing,
                    senderName = nameOf(r.senderHex),
                    withdrawn = gone,
                    reactedWith = reactions[r.senderHex to m.groupSeq].orEmpty(),
                    // The quote is the reader's own copy of the target,
                    // resolved by (sender, counter) — never bytes from the
                    // wire, so an unsend stays an unsend here too.
                    answering = m.groupReSender?.let { rs ->
                        m.groupReSeq?.let { rq ->
                            val t = rows.firstOrNull {
                                it.senderHex == rs && it.message.groupSeq == rq
                            }
                            when {
                                t == null -> stringResource(R.string.chat_reply_to_gone)
                                (rs to rq) in unsent -> stringResource(R.string.chat_unsent)
                                t.message.body.isNotBlank() -> t.message.body
                                else -> stringResource(R.string.chat_reply_to_message)
                            }
                        }
                    },
                    onPress = { menuFor = r },
                )
                }
            }
        }
        menuFor?.let { r ->
            val target = "${r.senderHex}:${r.message.groupSeq}"
            AlertDialog(
                onDismissRequest = { menuFor = null },
                title = { Text(stringResource(R.string.chat_message_title)) },
                text = {
                    Column {
                        Row {
                            listOf("👍", "❤️", "😂", "😮", "😢", "🔥").forEach { emo ->
                                Text(
                                    emo,
                                    style = MaterialTheme.typography.headlineSmall,
                                    modifier = Modifier
                                        .clickable {
                                            menuFor = null
                                            send(context.getString(R.string.chat_what_reaction)) {
                                                Groups.send(
                                                    context, idHex, emo, kind = 4,
                                                    reSender = r.senderHex,
                                                    reSeq = r.message.groupSeq,
                                                )
                                            }
                                        }
                                        .padding(4.dp),
                                )
                            }
                        }
                    }
                },
                confirmButton = {
                    Row {
                        TextButton(onClick = {
                            replyTo = target; menuFor = null
                        }) { Text(stringResource(R.string.chat_reply)) }
                        // Only the author's own words, and only once: the far
                        // side honours a withdrawal only from the message's
                        // sender, so offering it elsewhere would be a button
                        // that does nothing everywhere but here.
                        if (r.message.outgoing &&
                            (r.senderHex to r.message.groupSeq) !in unsent
                        ) {
                            TextButton(onClick = {
                                menuFor = null
                                send(null) {
                                    Groups.send(
                                        context, idHex,
                                        context.getString(R.string.chat_unsent),
                                        kind = 5,
                                        reSender = r.senderHex,
                                        reSeq = r.message.groupSeq,
                                    )
                                }
                            }) { Text(stringResource(R.string.chat_unsend)) }
                        }
                    }
                },
                dismissButton = {
                    TextButton(onClick = { menuFor = null }) {
                        Text(stringResource(R.string.chat_cancel))
                    }
                },
            )
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
                // The same reading the bubbles give a quote: withdrawn words
                // stay withdrawn in the composer too.
                Text(
                    if ((rs to rq) in unsent) stringResource(R.string.chat_unsent)
                    else rows.firstOrNull { it.senderHex == rs && it.message.groupSeq == rq }
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
            // Splitting is a kind of speaking: it announces arithmetic to the
            // group and bills people pairwise, so it sits behind the same mesh
            // gate as the composer. A split you cannot announce is a set of
            // bills nobody can check against each other.
            IconButton(
                onClick = { splitOpen = true },
                enabled = missing.isEmpty(),
            ) {
                Icon(Icons.AutoMirrored.Filled.CallSplit, stringResource(R.string.group_split))
            }
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
                    // The box keeps the words until the fan-out lands;
                    // the tick effect above empties it then, or hands
                    // them back if nothing left the phone.
                    ThreadSends.launch(store, key, null, body) {
                        val all = Groups.send(
                            context, idHex, body,
                            reSender = target?.get(0),
                            reSeq = target?.get(1)?.toLong(),
                        )
                        if (all) null else context.getString(R.string.group_partial_queued)
                    }
                },
                enabled = missing.isEmpty() && draft.isNotBlank() && !sending,
            ) {
                if (sending) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                else Icon(Icons.AutoMirrored.Filled.Send, stringResource(R.string.chat_send))
            }
        }
    }

    if (splitOpen) {
        SplitSheet(idHex = idHex, group = group, onDone = { splitOpen = false })
    }
    if (addOpen) {
        // This persona's contacts, not the phone's: see Groups.mineIn.
        val candidates = remember(contacts, group) {
            val personas = PersonaStore(context)
            val asWhom = Groups.mineIn(context, group)
            contacts.filter {
                it.personaHex !in group.members && personas.ownerHexOf(it) == asWhom
            }
        }
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
                                    // Under the thread's sends, not the
                                    // screen's scope: a rotation right after
                                    // the tap cancelled the launch before it
                                    // ran, and nobody was added. Groups.add
                                    // queues any roster copy that does not
                                    // go, so what it throws is a real fault
                                    // — shown as one, not swallowed.
                                    send(context.getString(R.string.chat_what_invite)) {
                                        Groups.add(context, idHex, c.personaHex)
                                        true
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
    withdrawn: Boolean,
    reactedWith: List<String>,
    answering: String?,
    onPress: () -> Unit,
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
                .clickable(onClick = onPress),
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
                if (withdrawn) {
                    Text(
                        stringResource(R.string.chat_unsent),
                        color = fg.copy(alpha = 0.7f),
                        fontStyle = FontStyle.Italic,
                    )
                } else {
                    Text(isolate(m.body), color = fg)
                }
            }
        }
        if (reactedWith.isNotEmpty()) {
            Text(
                reactedWith.joinToString(" "),
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(horizontal = 6.dp),
            )
        }
    }
}

/**
 * Making a group: a name, and members picked from *contacts only* — the list
 * is the constraint made visible, since fan-out can only ever reach people
 * this phone already holds. The disclosure fires on the group's first open.
 *
 * A screen of its own, like the group it makes: drawn inside the Chats tab
 * it sat under the tab's bar with its own title beneath, and the back
 * gesture — the tab shell's — left the half-filled form for Home.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun GroupCreateScreen(onDone: (String) -> Unit, onCancel: () -> Unit) {
    val context = LocalContext.current
    // The worn persona's contacts, because the group is made as the worn
    // persona (Groups.create) and a member has to be somebody it holds —
    // see Groups.mineIn. The single-persona era sees the whole book.
    val contacts = remember {
        val personas = PersonaStore(context)
        val all = ContactStore(context).all()
        if (personas.all().size > 1) {
            val worn = personas.worn()
            all.filter { personas.ownerHexOf(it) == worn }
        } else all
    }
    var name by rememberSaveable { mutableStateOf("") }
    // The picks ride the rotation with the name; a set does not fit a
    // Bundle as itself, a list of its members does.
    var picked by rememberSaveable(
        stateSaver = androidx.compose.runtime.saveable.listSaver<Set<String>, String>(
            save = { it.toList() }, restore = { it.toSet() },
        ),
    ) { mutableStateOf(setOf<String>()) }
    // Making the group is a send — the roster to every member, as long as
    // the network takes — and it ran on this screen's scope: turn the phone
    // while the spinner was up and the form came back with the name, the
    // picks and a live button, the group already made. A second tap made
    // it twice. Under ThreadSends now, keyed by a ticket that rides the
    // rotation, so the form that comes back finds the creation still going
    // and then the id to open.
    val ticket = rememberSaveable { java.util.UUID.randomUUID().toString() }
    val key = "create:$ticket"
    var busy by remember { mutableStateOf(ThreadSends.inFlight(key)) }
    var error by remember { mutableStateOf<String?>(null) }
    val tick by ThreadSends.ticks.collectAsState()
    LaunchedEffect(tick) {
        busy = ThreadSends.inFlight(key)
        for (o in ThreadSends.take(key)) when (o) {
            is ThreadSends.Outcome.Landed -> o.result?.let(onDone)
            // Used to be swallowed whole: the spinner stopped and the form
            // sat there, made of nothing.
            is ThreadSends.Outcome.Failed -> error = moneyFailure(context, o.error)
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.background,
                ),
                title = { Text(stringResource(R.string.group_create_title)) },
                navigationIcon = {
                    IconButton(onClick = onCancel) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = stringResource(R.string.chat_back),
                        )
                    }
                },
            )
        },
    ) { padding ->
        // A pick that outlived its contact (deleted while the form was in
        // the bundle) is not a member; the button counts what can be seated.
        val chosen = picked.filter { p -> contacts.any { it.personaHex == p } }
        Column(
            Modifier.fillMaxSize().padding(padding)
                .padding(start = 16.dp, end = 16.dp, bottom = 16.dp),
        ) {
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
            error?.let {
                Text(
                    it, Modifier.padding(bottom = 8.dp),
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            Row {
                OutlinedButton(onClick = onCancel, modifier = Modifier.weight(1f)) {
                    Text(stringResource(R.string.chat_cancel))
                }
                Spacer(Modifier.width(12.dp))
                Button(
                    onClick = {
                        busy = true; error = null
                        val title = name.trim()
                        val members = chosen
                        ThreadSends.launch(ContactStore(context), key, null) {
                            Groups.create(context, title, members).idHex
                        }
                    },
                    enabled = !busy && name.isNotBlank() && chosen.size >= 2,
                    modifier = Modifier.weight(1f),
                ) {
                    if (busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                    else Text(stringResource(R.string.group_create_button))
                }
            }
        }
    }
}

/**
 * Split a bill (§16.19's one brush with money — and deliberately not a wire
 * feature). One person fronted a total; this mints an ordinary pairwise
 * PaymentRequest to each chosen member for their share, then says the
 * arithmetic aloud in the group so everyone can check everyone's bill against
 * the same sentence. Money stays pairwise: the group carries only words.
 *
 * The share rounds DOWN. The splitter fronted the bill and eats the dust,
 * because a split that bills a friend a piconero over their share is wrong in
 * the direction that matters — same rule as [org.ducatproject.ducat.Tax.on].
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SplitSheet(idHex: String, group: Groups.Group, onDone: () -> Unit) {
    val context = LocalContext.current
    val store = remember { ContactStore(context) }
    val mine = remember { PersonaStore(context).allHexes() }
    val contacts = remember { store.all() }
    val youLabel = stringResource(R.string.group_you)
    fun nameOf(hex: String): String = when {
        hex in mine -> youLabel
        else -> contacts.firstOrNull { it.personaHex == hex }?.displayName()
            ?: "${hex.take(8)}…"
    }

    var typed by rememberSaveable { mutableStateOf("") }
    var fiatEntry by rememberSaveable {
        mutableStateOf(org.ducatproject.ducat.Amounts.enterFiat(context))
    }
    val rate = remember { org.ducatproject.ducat.RateStore(context).cached()?.first }
    val cur = remember { org.ducatproject.ducat.Amounts.currency(context) }
    var note by rememberSaveable { mutableStateOf("") }
    // Who shares the bill — everyone until unchecked, yourself included.
    // Unchecking yourself is "I didn't eat": the total splits among the rest.
    val checked = rememberSaveable(saver = HEX_LIST_SAVER) {
        mutableStateListOf<String>().apply { addAll(group.members) }
    }
    var error by rememberSaveable { mutableStateOf<String?>(null) }
    // Debtors already billed. A partial failure retries only the rest —
    // re-sending a bill someone already has is a duplicate, not a retry.
    // Saved with the amount: this list is the one thing a rotation must
    // not lose, since the bills it names are in people's threads already.
    val sent = rememberSaveable(saver = HEX_LIST_SAVER) { mutableStateListOf<String>() }
    val locked = sent.isNotEmpty()
    // The bills go out one member at a time and then the sentence to the
    // group — as long as the network takes, times the table. Under
    // ThreadSends so the phone turning mid-way neither loses the busy
    // state nor the answer: whichever sheet is up when it comes reads it.
    val key = "split:$idHex"
    var busy by remember { mutableStateOf(ThreadSends.inFlight(key)) }
    val tick by ThreadSends.ticks.collectAsState()
    LaunchedEffect(tick) {
        busy = ThreadSends.inFlight(key)
        for (o in ThreadSends.take(key)) when (o) {
            is ThreadSends.Outcome.Landed -> onDone()
            is ThreadSends.Outcome.Failed -> {
                val p = o.error as? SplitPartial
                if (p != null) {
                    sent.addAll(p.billed.filter { it !in sent })
                    error = context.getString(R.string.group_split_partial, p.fails.joinToString(", "))
                } else {
                    error = moneyFailure(context, o.error)
                }
            }
        }
    }

    val pxmr = remember(typed, fiatEntry, rate) {
        val v = moneyText(typed).toBigDecimalOrNull() ?: return@remember null
        val xmr = if (fiatEntry && rate != null && rate > 0) {
            v.divide(java.math.BigDecimal.valueOf(rate), 12, java.math.RoundingMode.DOWN)
        } else v
        // Amounts.toPxmr, which is longValueExact and returns null rather
        // than wrapping. `.toLong()` is longValue(): it keeps the low 64
        // bits, so a pasted 2^64+1 came through as exactly 1 XMR and the
        // sheet split a number nobody typed.
        Amounts.toPxmr(xmr)?.takeIf { it > 0 }
    }
    val share = if (pxmr != null && checked.isNotEmpty()) pxmr / checked.size else null
    val debtors = checked.filter { it !in mine }

    // Resolved here because plurals are composition-only: the sentence the
    // group hears, e.g. "dinner — USD 20.00 · 3 ways — USD 6.67 each".
    val announce = if (pxmr != null && share != null) {
        note.ifBlank { stringResource(R.string.pay_payment_request) } +
            " — " + org.ducatproject.ducat.Amounts.show(context, pxmr).primary +
            " · " + pluralStringResource(
                R.plurals.group_split_ways, checked.size, checked.size,
                org.ducatproject.ducat.Amounts.show(context, share).primary,
            )
    } else ""
    val defaultNote = stringResource(R.string.pay_payment_request)

    ModalBottomSheet(onDismissRequest = { if (!busy) onDone() }) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp).padding(bottom = 24.dp)) {
            Text(stringResource(R.string.group_split), style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(12.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                OutlinedTextField(
                    value = typed,
                    onValueChange = {
                        typed = it.filter { c -> org.ducatproject.ducat.Amounts.isNumberChar(c) }
                    },
                    placeholder = { Text(stringResource(R.string.pay_amount_placeholder)) },
                    singleLine = true,
                    enabled = !locked && !busy,
                    keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                        keyboardType = androidx.compose.ui.text.input.KeyboardType.Decimal,
                    ),
                    modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.width(10.dp))
                if (rate != null) {
                    AssistChip(
                        onClick = { if (!locked && !busy) { fiatEntry = !fiatEntry; typed = "" } },
                        label = { Text(if (fiatEntry) cur else "XMR") },
                    )
                }
            }
            Spacer(Modifier.height(8.dp))
            OutlinedTextField(
                value = note,
                onValueChange = { if (it.length <= 128) note = it },
                label = { Text(stringResource(R.string.pay_memo_label)) },
                singleLine = true,
                enabled = !locked && !busy,
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(12.dp))
            Text(
                stringResource(R.string.group_split_among),
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            group.members.forEach { hex ->
                Row(
                    Modifier.fillMaxWidth().clickable(enabled = !locked && !busy) {
                        if (hex in checked) checked.remove(hex) else checked.add(hex)
                    },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Checkbox(
                        checked = hex in checked,
                        onCheckedChange = {
                            if (hex in checked) checked.remove(hex) else checked.add(hex)
                        },
                        enabled = !locked && !busy,
                    )
                    Text(isolate(nameOf(hex)))
                }
            }
            if (share != null && debtors.isNotEmpty()) {
                Spacer(Modifier.height(4.dp))
                Text(
                    pluralStringResource(
                        R.plurals.group_split_ways, checked.size, checked.size,
                        org.ducatproject.ducat.Amounts.show(context, share).primary,
                    ),
                    style = MaterialTheme.typography.titleMedium,
                )
            }
            error?.let {
                Spacer(Modifier.height(4.dp))
                Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
            }
            Spacer(Modifier.height(12.dp))
            Button(
                onClick = {
                    val shareNow = share ?: return@Button
                    busy = true; error = null
                    val already = sent.toList()
                    val memo = note.ifBlank { defaultNote }
                    val sentence = announce
                    ThreadSends.launch(store, key, context.getString(R.string.group_split)) {
                        val fails = ArrayList<String>()
                        val billed = ArrayList<String>()
                        for (d in debtors) {
                            if (d in already) continue
                            val c = contacts.firstOrNull { it.personaHex == d }
                            if (c == null) { fails.add(nameOf(d)); continue }
                            runCatching {
                                org.ducatproject.ducat.Mailbox.send(
                                    context, c, memo,
                                    kind = 1, amountPxmr = shareNow,
                                    payto = org.ducatproject.ducat.WalletStore(context)
                                        .addressFor(d),
                                )
                            }.onSuccess { billed.add(d) }.onFailure { fails.add(nameOf(d)) }
                        }
                        if (fails.isEmpty()) {
                            // The bills are out; now the sentence everyone
                            // can audit them against. If this fan-out only
                            // partially lands, Groups queues the rest —
                            // that failure mode is already handled below us.
                            runCatching { Groups.send(context, idHex, sentence) }
                                .onFailure { fails.add(group.name) }
                        }
                        if (fails.isNotEmpty()) throw SplitPartial(fails, billed)
                        null
                    }
                },
                enabled = !busy && share != null && debtors.isNotEmpty(),
                modifier = Modifier.fillMaxWidth(),
            ) {
                if (busy) CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                else Text(
                    pluralStringResource(
                        R.plurals.group_split_send,
                        (debtors.size - sent.size).coerceAtLeast(1),
                        (debtors.size - sent.size).coerceAtLeast(1),
                    ),
                )
            }
        }
    }
}
