package org.ducatproject.ducat.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.*
import androidx.compose.runtime.*
import kotlinx.coroutines.withContext
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import android.content.Context
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.SafeImage
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.R
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.draw.clip
import org.ducatproject.ducat.StoredMessage
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable

/**
 * The chat tab: conversations, not people.
 *
 * The distinction is deliberate and visible. Removing a conversation should not
 * throw away the person — you may still want to pay them — and forgetting the
 * person is a heavier action that belongs in Contacts, behind a confirmation
 * that says what it destroys.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
fun ChatListScreen(personaSecret: ByteArray?, onOpenChat: (Contact) -> Unit) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val store = remember { ContactStore(context) }
    var all by remember { mutableStateOf(store.all()) }
    // Recomputed with the list, since adding or renaming a contact is
    // exactly what makes two of them read the same. Off-main with the rest:
    // it re-reads the whole book.
    var ambiguous by remember { mutableStateOf<Set<String>>(emptySet()) }
    LaunchedEffect(all) {
        ambiguous = withContext(kotlinx.coroutines.Dispatchers.IO) { store.ambiguous() }
    }
    // Same reason as the chat screen: a message arriving must move this list,
    // and nothing else tells it one did.
    val version by ContactStore.changes.collectAsState()
    LaunchedEffect(version) { all = store.all() }
    var sheet by remember { mutableStateOf<Sheet?>(null) }
    var confirm by remember { mutableStateOf<Contact?>(null) }
    // §16.19: the groups, above the pairwise threads they fan into.
    val groups = remember(version) { org.ducatproject.ducat.Groups.all(context) }
    var openGroup by rememberSaveable { mutableStateOf<String?>(null) }
    var newGroup by remember { mutableStateOf(false) }

    val og = openGroup
    if (og != null) {
        GroupChatScreen(idHex = og, onBack = { openGroup = null })
        return
    }
    if (newGroup) {
        GroupCreateScreen(
            onDone = { made -> newGroup = false; openGroup = made },
            onCancel = { newGroup = false },
        )
        return
    }

    // Most recent conversation first — the list's order *is* its meaning, and
    // "who did I talk to last" is the question it answers. Threads that have
    // never spoken sink to the bottom together.
    // The sort decodes every thread to find its last human-visible line —
    // per contact, per store bump — so it runs on IO and lands as state
    // (the ledger ANR's lesson). The list is briefly stale, never frozen.
    var shown by remember { mutableStateOf<List<Contact>>(emptyList()) }
    // The same pass that sorts also keeps each row's last line and unread
    // flag: the rows used to re-decode their thread inside composition (a
    // full decrypt each, keyed on `all` so every store bump redid all of
    // them) and read chatSeen — another decrypt — per frame. One walk, on
    // IO, keyed on version so a read-marker write moves the dots too.
    var rowLast by remember { mutableStateOf<Map<String, StoredMessage?>>(emptyMap()) }
    var rowUnread by remember { mutableStateOf<Set<String>>(emptySet()) }
    LaunchedEffect(version, all) {
        val (sorted, lasts, unread) = withContext(kotlinx.coroutines.Dispatchers.IO) {
            // The worn compartment's conversations only, once a second
            // persona exists — the switcher in the drawer is how the others
            // are reached. One hat, one list; the single-persona era sees
            // no change because every owner resolves to the primary.
            val personas = PersonaStore(context)
            val worn = personas.worn()
            val scoped = if (personas.all().size > 1) {
                all.filter { personas.ownerHexOf(it) == worn }
            } else all
            val visible = scoped.filter { it.chatVisible }
            // By the last message a person could have read, not the last
            // one the protocol wrote. Calling off an escrow sends a kind
            // 10, and that alone lifted a dormant arbiter to the top of
            // the list above conversations with actual sentences in them.
            val lasts = visible.associate { c ->
                c.personaHex to store.thread(c.personaHex)
                    .lastOrNull { it.kind !in CEREMONY_KINDS && it.groupId == null }
            }
            val unread = visible
                .filter { it.inSeq > store.chatSeen(it.personaHex) }
                .map { it.personaHex }.toSet()
            Triple(
                visible.sortedByDescending { lasts[it.personaHex]?.timestamp ?: 0L },
                lasts, unread,
            )
        }
        shown = sorted; rowLast = lasts; rowUnread = unread
    }
    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(16.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Button(
                onClick = { sheet = Sheet.Share },
                modifier = Modifier.weight(1f),
                enabled = personaSecret != null,
            ) {
                Icon(Icons.Filled.Share, null, Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text(stringResource(R.string.chatlist_my_card))
            }
            OutlinedButton(onClick = { sheet = Sheet.New }, modifier = Modifier.weight(1f)) {
                Icon(Icons.Filled.ChatBubbleOutline, null, Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text(stringResource(R.string.chatlist_new_chat))
            }
        }
        // **Words, found again.** The Activity tab searches what money said;
        // this searches what people said — every thread and every group, by
        // body and by attachment name, because "where did she send that
        // address" has had no answer but scrolling. Local only, like the
        // threads themselves.
        var search by rememberSaveable { mutableStateOf("") }
        OutlinedTextField(
            value = search,
            onValueChange = { search = it },
            singleLine = true,
            placeholder = { Text(stringResource(R.string.chatlist_search_hint)) },
            leadingIcon = { Icon(Icons.Filled.Search, null) },
            trailingIcon = {
                if (search.isNotEmpty()) {
                    IconButton(onClick = { search = "" }) {
                        Icon(Icons.Filled.Close, stringResource(R.string.activity_search_clear))
                    }
                }
            },
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        )
        Spacer(Modifier.height(4.dp))
        val q = search.trim().lowercase()
        if (q.isNotEmpty()) {
            // One row per hit: who, the line it matched, when. Tapping goes to
            // the conversation it lives in — the row is a pointer, not a copy.
            data class Hit(
                val label: String, val snippet: String, val ts: Long?,
                val open: () -> Unit,
            )
            // Names first: someone typing "ladder" wants the ladder-crew
            // group, whether or not anyone has said the word aloud. These
            // rows carry no timestamp — they name a place, not a moment.
            val nameHits = buildList {
                for (g in groups) {
                    if (q !in g.name.lowercase()) continue
                    add(Hit(
                        g.name,
                        pluralStringResource(
                            R.plurals.group_members, g.members.size, g.members.size,
                        ),
                        null,
                    ) { openGroup = g.idHex })
                }
                for (c in all) {
                    if (q !in c.displayName().lowercase()) continue
                    add(Hit(c.displayName(), "", null) { onOpenChat(c) })
                }
            }
            var bodyHits by remember { mutableStateOf<List<Hit>>(emptyList()) }
            LaunchedEffect(q, version) {
                // Restarting on every keystroke is the debounce: the delay
                // dies with the superseded effect, and only a pause reads.
                kotlinx.coroutines.delay(120)
                bodyHits = withContext(kotlinx.coroutines.Dispatchers.IO) {
                val out = ArrayList<Hit>()
                for (c in all) {
                    for (m in store.thread(c.personaHex)) {
                        if (m.kind !in setOf(0, 1, 2, 3) || m.groupId != null) continue
                        val hay = (m.body + " " + (m.attName ?: "")).lowercase()
                        if (q !in hay) continue
                        out.add(Hit(c.displayName(), m.body.ifBlank { m.attName ?: "" }, m.timestamp) {
                            onOpenChat(c)
                        })
                    }
                }
                for (g in groups) {
                    for (r in org.ducatproject.ducat.Groups.thread(context, g.idHex)) {
                        val m = r.message
                        if (m.kind != 0) continue
                        if (q !in m.body.lowercase()) continue
                        out.add(Hit(g.name, m.body, m.timestamp) { openGroup = g.idHex })
                    }
                }
                out.sortedByDescending { it.ts ?: 0L }.take(50)
                }
            }
            val hits = nameHits + bodyHits
            if (hits.isEmpty()) {
                Text(
                    stringResource(R.string.activity_search_none, search),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(16.dp),
                )
            } else {
                LazyColumn(Modifier.fillMaxSize()) {
                    items(hits.size) { i ->
                        val h = hits[i]
                        ListItem(
                            modifier = Modifier.clickable { h.open() },
                            colors = ListItemDefaults.colors(
                                containerColor = androidx.compose.ui.graphics.Color.Transparent,
                            ),
                            headlineContent = { Text(isolate(h.label)) },
                            supportingContent = h.snippet.takeIf { it.isNotEmpty() }?.let {
                                {
                                    Text(
                                        isolate(it), maxLines = 2,
                                        overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                                    )
                                }
                            },
                            trailingContent = h.ts?.let {
                                {
                                    Text(
                                        clockTime(context, it),
                                        style = MaterialTheme.typography.labelSmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            },
                        )
                    }
                }
            }
            return
        }
        // Groups, each by name with its size. Rendered above the threads they
        // fan into so the two lists cannot be confused for one.
        if (groups.isNotEmpty() || ContactStore(context).all().size >= 2) {
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    stringResource(R.string.group_section),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.weight(1f))
                TextButton(onClick = { newGroup = true }) {
                    Text(stringResource(R.string.group_new))
                }
            }
            groups.forEach { g ->
                ListItem(
                    modifier = Modifier.clickable { openGroup = g.idHex },
                    colors = ListItemDefaults.colors(
                        containerColor = androidx.compose.ui.graphics.Color.Transparent,
                    ),
                    leadingContent = { Avatar(g.name, null) },
                    headlineContent = { Text(isolate(g.name)) },
                    supportingContent = {
                        Text(
                            pluralStringResource(
                                R.plurals.group_members, g.members.size, g.members.size,
                            ),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    },
                )
            }
            HorizontalDivider(
                color = MaterialTheme.colorScheme.outlineVariant,
                modifier = Modifier.padding(vertical = 4.dp),
            )
        }

        if (shown.isEmpty()) {
            Column(
                Modifier.fillMaxWidth().padding(32.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Icon(
                    Icons.Filled.ChatBubbleOutline, null, Modifier.size(48.dp),
                    tint = MaterialTheme.colorScheme.outline,
                )
                Spacer(Modifier.height(12.dp))
                Text(
                    stringResource(R.string.chatlist_no_conversations),
                    style = MaterialTheme.typography.titleMedium,
                )
                Spacer(Modifier.height(6.dp))
                Text(
                    stringResource(R.string.chatlist_empty_hint),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else {
            LazyColumn(Modifier.fillMaxSize()) {
                items(shown, key = { it.personaHex }) { c ->
                    // The newest message a person would recognise. Ceremony
                    // traffic is the last thing in a thread whenever an escrow
                    // is mid-flight, and as a preview it reads as "bond: your
                    // share" — internal words, about nothing the reader can
                    // act on. The abort (kind 10) was left out of the list
                    // when the other two went in, so calling a deal off put
                    // "You: ceremony: called off" under the other person's
                    // name — the one ceremony message a *person* sends, and
                    // the one that therefore reaches the list.
                    val last = rowLast[c.personaHex]
                    val unread = c.personaHex in rowUnread
                    val unreadLabel = stringResource(R.string.chatlist_unread)
                    val deleteLabel = stringResource(R.string.chatlist_delete_long)
                    ListItem(
                        colors = ListItemDefaults.colors(containerColor = androidx.compose.ui.graphics.Color.Transparent),
                        headlineContent = {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Text(
                                    c.displayName(),
                                    fontWeight = if (unread) FontWeight.Bold else FontWeight.Normal,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                    modifier = Modifier.weight(1f, fill = false),
                                )
                                // Beside the name, not under it: the line below
                                // is the last message, which is why anybody is
                                // looking at this row. An icon rather than
                                // words because the row is already full — the
                                // sentence is on the screens that spend money.
                                if (c.personaHex in ambiguous) {
                                    Spacer(Modifier.width(6.dp))
                                    Icon(
                                        Icons.Filled.Warning,
                                        stringResource(R.string.chatlist_name_shared),
                                        tint = MaterialTheme.colorScheme.error,
                                        modifier = Modifier.size(16.dp),
                                    )
                                }
                            }
                        },
                        supportingContent = {
                            Text(
                                last?.let {
                                    val preview = previewOf(context, it)
                                    if (it.outgoing) {
                                        stringResource(R.string.chatlist_preview_you, preview)
                                    } else preview
                                } ?: stringResource(R.string.chatlist_no_messages_yet),
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                style = MaterialTheme.typography.bodySmall,
                                fontWeight = if (unread) FontWeight.SemiBold else FontWeight.Normal,
                            )
                        },
                        leadingContent = { Avatar(c.displayName(), c.avatar) },
                        trailingContent = {
                            Column(horizontalAlignment = Alignment.End) {
                                last?.let {
                                    Text(
                                        shortWhen(context, it.timestamp),
                                        style = MaterialTheme.typography.labelSmall,
                                        color = if (unread) MaterialTheme.colorScheme.primary
                                        else MaterialTheme.colorScheme.outline,
                                    )
                                }
                                if (unread) {
                                    Spacer(Modifier.height(4.dp))
                                    Box(
                                        Modifier.size(9.dp).background(
                                            MaterialTheme.colorScheme.primary, CircleShape,
                                        )
                                    )
                                }
                            }
                        },
                        // Delete moved to long-press: a trash can on every row
                        // is one mis-tap from losing a thread, and the dialog
                        // it opened was the only thing between. The long-press
                        // label and the unread state are for screen readers —
                        // the dot is 9dp of colour, and colour is the one
                        // channel a reader does not carry.
                        modifier = Modifier
                            .semantics {
                                if (unread) stateDescription = unreadLabel
                            }
                            .combinedClickable(
                                onClick = { onOpenChat(c) },
                                onLongClickLabel = deleteLabel,
                                onLongClick = { confirm = c },
                            ),
                    )
                }
            }
        }
    }

    confirm?.let { c ->
        AlertDialog(
            onDismissRequest = { confirm = null },
            title = { Text(stringResource(R.string.chatlist_delete_title)) },
            text = {
                Text(stringResource(R.string.chatlist_delete_body, isolate(c.displayName())))
            },
            confirmButton = {
                TextButton(onClick = {
                    store.deleteThread(c.personaHex)
                    store.setChatVisible(c.personaHex, false)
                    all = store.all()
                    confirm = null
                }) {
                    Text(
                        stringResource(R.string.chatlist_delete),
                        color = MaterialTheme.colorScheme.error,
                    )
                }
            },
            dismissButton = {
                TextButton(onClick = { confirm = null }) {
                    Text(stringResource(R.string.chatlist_cancel))
                }
            },
        )
    }

    when (sheet) {
        Sheet.Share -> ShareCardSheet(personaSecret) { sheet = null }
        Sheet.Add -> AddContactSheet(
            onDismiss = { sheet = null },
            onAdded = { all = store.all(); sheet = null },
            store = store,
        )
        Sheet.New -> NewChatSheet(
            contacts = all.sortedBy { it.displayName().lowercase() },
            ambiguous = store.ambiguous(),
            onDismiss = { sheet = null },
            onAdd = { sheet = Sheet.Add },
            onPick = {
                store.setChatVisible(it.personaHex, true)
                all = store.all()
                sheet = null
                onOpenChat(it)
            },
        )
        null -> {}
    }
}

internal enum class Sheet { Share, Add, New }

/**
 * A conversation starts from a person, and every contact qualifies — including
 * ones whose chat was deleted (the relationship outlives its messages) and
 * ones that arrived through a sale rather than a hello.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun NewChatSheet(
    contacts: List<Contact>,
    ambiguous: Set<String>,
    onDismiss: () -> Unit,
    onAdd: () -> Unit,
    onPick: (Contact) -> Unit,
) {
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(Modifier.padding(bottom = 24.dp)) {
            Text(
                stringResource(R.string.chatlist_new_chat),
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(20.dp),
            )
            ListItem(
                colors = ListItemDefaults.colors(
                    containerColor = androidx.compose.ui.graphics.Color.Transparent,
                ),
                headlineContent = { Text(stringResource(R.string.chatlist_add_contact)) },
                supportingContent = {
                    Text(stringResource(R.string.chatlist_add_contact_hint))
                },
                leadingContent = {
                    Icon(
                        Icons.Filled.PersonAdd, null,
                        tint = MaterialTheme.colorScheme.primary,
                    )
                },
                modifier = Modifier.clickable(onClick = onAdd),
            )
            if (contacts.isEmpty()) {
                Text(
                    stringResource(R.string.chatlist_no_contacts_yet),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp),
                )
            }
            contacts.forEach { c ->
                ListItem(
                    colors = ListItemDefaults.colors(
                        containerColor = androidx.compose.ui.graphics.Color.Transparent,
                    ),
                    headlineContent = { Text(c.displayName()) },
                    // Of every list in the app this is the one with nothing but
                    // a name on it, so a name two people share leaves nothing
                    // to choose by. The key only appears where it is needed.
                    supportingContent = if (c.personaHex !in ambiguous) null else {
                        {
                            Text(
                                stringResource(
                                    R.string.chatlist_name_shared_key, c.personaHex.take(16),
                                ),
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.error,
                            )
                        }
                    },
                    leadingContent = { Avatar(c.displayName(), c.avatar) },
                    modifier = Modifier.clickable { onPick(c) },
                )
            }
        }
    }
}

/**
 * Their face if they published one, their initial if not.
 *
 * The bytes came off a contact record written by someone else, so decoding is
 * wrapped: a picture that will not parse falls back to the letter rather than
 * taking the list down with it. Being unable to draw somebody's avatar is not
 * a reason to be unable to draw the conversation.
 */
@Composable
internal fun Avatar(name: String, picture: ByteArray? = null, size: Int = 40) {
    val bmp = remember(picture) {
        picture?.let {
            SafeImage.fromBytes(it, SafeImage.AVATAR_PIXELS)
        }
    }
    Box(
        Modifier
            .size(size.dp)
            .clip(RoundedCornerShape(size.dp / 2))
            .background(MaterialTheme.colorScheme.secondaryContainer),
        contentAlignment = Alignment.Center,
    ) {
        if (bmp != null) {
            androidx.compose.foundation.Image(
                bmp.asImageBitmap(), null,
                Modifier.fillMaxSize(),
                contentScale = androidx.compose.ui.layout.ContentScale.Crop,
            )
        } else {
            Text(
                // The first letter, not the first character. Plenty of people
                // put a mark before their name — ".kara" drew a full stop in
                // a circle, which is not an initial and not a face.
                (name.firstOrNull { it.isLetterOrDigit() } ?: name.firstOrNull() ?: '?')
                    .uppercase(),
                color = MaterialTheme.colorScheme.onSecondaryContainer,
                fontWeight = FontWeight.Bold,
            )
        }
    }
}


/**
 * The message kinds that are machinery rather than conversation: the DKG and
 * FROST rounds, the abort that ends a ceremony, and the position reference
 * that hands over a live-position stream — the app writes that one, not a
 * person, and the ride's own card is what somebody reads. Chat.kt filters the same
 * set out of the thread itself; here it keeps them out of the preview line
 * and out of the order the list is sorted in.
 */
private val CEREMONY_KINDS = setOf(8, 9, 10, 11, 12)

/** What one message looks like from a list away (§16.13's kinds included). */
internal fun previewOf(context: Context, m: StoredMessage): String = when {
    // A gap, said the way the thread says it. These are placeholders the
    // reader wrote, and their bodies are written for a bubble rather than a
    // list — "[a message could not be opened — it was sealed to a key this
    // device no longer holds]" is the whole preview line and then some, and
    // it reads like an error escaping. A restored phone can have a run of
    // them as the newest thing in a thread.
    m.deadLetter ->
        context.resources.getQuantityString(R.plurals.chat_gap_unread, 1, 1)
    m.kind == 1 ->
        context.getString(
            R.string.chatlist_preview_requested,
            Amounts.show(context, m.amountPxmr).primary,
        )
    m.kind == 2 ->
        context.getString(
            R.string.chatlist_preview_sent,
            Amounts.show(context, m.amountPxmr).primary,
        )
    m.kind == 3 ->
        context.getString(
            R.string.chatlist_preview_receipt,
            Amounts.show(context, m.amountPxmr).primary,
        )
    m.kind == 4 -> context.getString(R.string.chatlist_preview_reacted, isolate(m.body))
    m.kind == 13 -> context.getString(
        R.string.chatlist_preview_issue, isolate(m.pubPeriodId ?: ""),
    ).trim()
    m.attHash != null -> context.getString(R.string.chatlist_preview_photo)
    else -> isolate(m.body)
}

/**
 * Fence text of unknown direction off from the paragraph around it.
 *
 * A message body is whatever somebody typed, in whatever script, and it gets
 * dropped into a UI whose direction belongs to the *reader*. Without a fence,
 * the bidi algorithm resolves the whole line together and the run's trailing
 * punctuation migrates to the paragraph's end — so an English sentence read in
 * Arabic came out as ".not this time", full stop first, and the retraction line
 * (Arabic label, English quote, quotation marks between them) scrambled outright.
 *
 * U+2068 FIRST STRONG ISOLATE opens a run whose direction is decided by its own
 * first strong character; U+2069 POP DIRECTIONAL ISOLATE closes it. The text is
 * unchanged — these are formatting characters, they do not print, and they do
 * not survive into anything copied out as plain content.
 */
internal fun isolate(s: String): String =
    if (s.isEmpty()) s else "⁨" + s + "⁩"

/**
 * How much room is left in a capped field, once the cap is in sight.
 *
 * Every text field in the app refuses input past its limit — a name at 32, an
 * item at 40, a till note at 64 — and refused it *silently*, so typing simply
 * stopped producing letters. That reads as the app having hung, not as a rule,
 * and the person retypes rather than shortening.
 *
 * Only near the end, rather than Material's always-on counter: a permanent
 * "3/32" under somebody's name is noise about a limit almost nobody meets.
 */
@Composable
internal fun CharCounter(length: Int, max: Int, within: Int = 10) {
    if (max - length > within) return
    Text(
        stringResource(R.string.field_counter, length, max),
        style = MaterialTheme.typography.labelSmall,
        color = if (length >= max) MaterialTheme.ducat.lowCapacity
        else MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

/**
 * A clock time, in the shape this phone's owner reads clock times.
 *
 * Not `SimpleDateFormat("HH:mm")`, which is what three screens used: `HH` is
 * 24-hour by pattern, so it stayed 24-hour no matter what the person had set,
 * and `Locale.getDefault()` does not change that — the setting is Android's
 * own, not the locale's. Somebody in a country that writes half past two as
 * 2:30 PM saw 14:30 under every message they had ever sent.
 *
 * The system formatter answers the setting, and answers it per-phone rather
 * than per-country, which is the way the question is actually asked.
 */
internal fun clockTime(context: Context, epochSecs: Long): String =
    android.text.format.DateFormat.getTimeFormat(context)
        .format(java.util.Date(epochSecs * 1000))

/** Now, minutes, hours, weekday, then a date — the resolution a list needs. */
internal fun shortWhen(context: Context, epochSecs: Long): String {
    val now = System.currentTimeMillis() / 1000
    val d = now - epochSecs
    return when {
        d < 60 -> context.getString(R.string.chatlist_time_now)
        d < 3600 -> context.getString(R.string.chatlist_time_minutes, d / 60)
        d < 86_400 -> context.getString(R.string.chatlist_time_hours, d / 3600)
        d < 7 * 86_400 -> java.text.SimpleDateFormat("EEE", java.util.Locale.getDefault())
            .format(java.util.Date(epochSecs * 1000))
        // Not a pinned "d MMM": day-before-month is an English order, and
        // Japanese, Chinese and Korean put the month first. The skeleton asks
        // the locale which way round it writes them — the same lesson as
        // clockTime above, one line up from where it was ignored.
        else -> java.text.SimpleDateFormat(
            android.text.format.DateFormat.getBestDateTimePattern(
                java.util.Locale.getDefault(), "dMMM"),
            java.util.Locale.getDefault(),
        ).format(java.util.Date(epochSecs * 1000))
    }
}
