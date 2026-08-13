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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore

/**
 * The chat tab: conversations, not people.
 *
 * The distinction is deliberate and visible. Removing a conversation should not
 * throw away the person — you may still want to pay them — and forgetting the
 * person is a heavier action that belongs in Contacts, behind a confirmation
 * that says what it destroys.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatListScreen(personaSecret: ByteArray?, onOpenChat: (Contact) -> Unit) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val store = remember { ContactStore(context) }
    var all by remember { mutableStateOf(store.all()) }
    var sheet by remember { mutableStateOf<Sheet?>(null) }
    var confirm by remember { mutableStateOf<Contact?>(null) }

    val shown = all.filter { it.chatVisible }
    val hidden = all.filterNot { it.chatVisible }

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
                Text("My card")
            }
            OutlinedButton(onClick = { sheet = Sheet.Add }, modifier = Modifier.weight(1f)) {
                Icon(Icons.Filled.PersonAdd, null, Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text("Add")
            }
        }

        if (hidden.isNotEmpty()) {
            TextButton(
                onClick = { sheet = Sheet.Restore },
                modifier = Modifier.padding(horizontal = 16.dp),
            ) {
                Icon(Icons.Filled.Restore, null, Modifier.size(16.dp))
                Spacer(Modifier.width(6.dp))
                Text("${hidden.size} hidden — start a chat again")
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
                Text("No conversations", style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(6.dp))
                Text(
                    "Hand someone your card in person, or send it over any app you " +
                        "already trust. Nobody can be found by name — reaching a " +
                        "person takes a card they gave you.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else {
            LazyColumn(Modifier.fillMaxSize()) {
                items(shown, key = { it.personaHex }) { c ->
                    val last = remember(c.personaHex, all) {
                        store.thread(c.personaHex).lastOrNull()
                    }
                    ListItem(
                        headlineContent = { Text(c.displayName()) },
                        supportingContent = {
                            Text(
                                last?.let { (if (it.outgoing) "You: " else "") + it.body }
                                    ?: "No messages yet",
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                style = MaterialTheme.typography.bodySmall,
                            )
                        },
                        leadingContent = { Avatar(c.displayName()) },
                        trailingContent = {
                            IconButton(onClick = { confirm = c }) {
                                Icon(Icons.Filled.DeleteOutline, "Delete conversation")
                            }
                        },
                        modifier = Modifier.clickable { onOpenChat(c) },
                    )
                    HorizontalDivider()
                }
            }
        }
    }

    confirm?.let { c ->
        AlertDialog(
            onDismissRequest = { confirm = null },
            title = { Text("Delete this conversation?") },
            text = {
                Text(
                    "Every message with ${c.displayName()} is deleted from this " +
                        "phone and cannot be recovered. ${c.displayName()} stays in " +
                        "your contacts, and you can start again any time."
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    store.deleteThread(c.personaHex)
                    store.setChatVisible(c.personaHex, false)
                    all = store.all()
                    confirm = null
                }) { Text("Delete", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = { confirm = null }) { Text("Cancel") } },
        )
    }

    when (sheet) {
        Sheet.Share -> ShareCardSheet(personaSecret) { sheet = null }
        Sheet.Add -> AddContactSheet(
            onDismiss = { sheet = null },
            onAdded = { all = store.all(); sheet = null },
            store = store,
        )
        Sheet.Restore -> RestoreChatSheet(
            hidden = hidden,
            onDismiss = { sheet = null },
            onPick = {
                store.setChatVisible(it.personaHex, true)
                all = store.all()
                sheet = null
            },
        )
        null -> {}
    }
}

internal enum class Sheet { Share, Add, Restore }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun RestoreChatSheet(
    hidden: List<Contact>,
    onDismiss: () -> Unit,
    onPick: (Contact) -> Unit,
) {
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(Modifier.padding(bottom = 24.dp)) {
            Text(
                "Start a chat again",
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(20.dp),
            )
            hidden.forEach { c ->
                ListItem(
                    headlineContent = { Text(c.displayName()) },
                    leadingContent = { Avatar(c.displayName()) },
                    modifier = Modifier.clickable { onPick(c) },
                )
            }
        }
    }
}

@Composable
internal fun Avatar(name: String) {
    Box(
        Modifier
            .size(40.dp)
            .background(MaterialTheme.colorScheme.secondaryContainer, RoundedCornerShape(20.dp)),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            name.take(1).uppercase(),
            color = MaterialTheme.colorScheme.onSecondaryContainer,
            fontWeight = FontWeight.Bold,
        )
    }
}
