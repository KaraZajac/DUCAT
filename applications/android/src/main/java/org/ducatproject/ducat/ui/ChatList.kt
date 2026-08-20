package org.ducatproject.ducat.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import android.content.Context
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
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
    // Same reason as the chat screen: a message arriving must move this list,
    // and nothing else tells it one did.
    val version by ContactStore.changes.collectAsState()
    LaunchedEffect(version) { all = store.all() }
    var sheet by remember { mutableStateOf<Sheet?>(null) }
    var confirm by remember { mutableStateOf<Contact?>(null) }

    // Most recent conversation first — the list's order *is* its meaning, and
    // "who did I talk to last" is the question it answers. Threads that have
    // never spoken sink to the bottom together.
    val shown = remember(all) {
        all.filter { it.chatVisible }.sortedByDescending { c ->
            store.thread(c.personaHex).lastOrNull()?.timestamp ?: 0L
        }
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
                    // rounds (8, 9) are the last thing in a thread whenever an
                    // escrow is mid-flight, and as a preview they read as
                    // "bond: your share" — internal words, about nothing the
                    // reader can act on. The thread still sorts by its real
                    // last message; only the line shown skips the machinery.
                    val last = remember(c.personaHex, all) {
                        store.thread(c.personaHex).lastOrNull { it.kind != 8 && it.kind != 9 }
                    }
                    val unread = c.inSeq > store.chatSeen(c.personaHex)
                    ListItem(
                        colors = ListItemDefaults.colors(containerColor = androidx.compose.ui.graphics.Color.Transparent),
                        headlineContent = {
                            Text(
                                c.displayName(),
                                fontWeight = if (unread) FontWeight.Bold else FontWeight.Normal,
                            )
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
                        // it opened was the only thing between.
                        modifier = Modifier.combinedClickable(
                            onClick = { onOpenChat(c) },
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
                Text(stringResource(R.string.chatlist_delete_body, c.displayName()))
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
            runCatching { android.graphics.BitmapFactory.decodeByteArray(it, 0, it.size) }
                .getOrNull()
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


/** What one message looks like from a list away (§16.13's kinds included). */
internal fun previewOf(context: Context, m: StoredMessage): String = when {
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
    m.kind == 4 -> context.getString(R.string.chatlist_preview_reacted, m.body)
    m.attHash != null -> context.getString(R.string.chatlist_preview_photo)
    else -> m.body
}

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
        else -> java.text.SimpleDateFormat("d MMM", java.util.Locale.getDefault())
            .format(java.util.Date(epochSecs * 1000))
    }
}
