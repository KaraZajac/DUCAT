package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import androidx.compose.foundation.clickable

/**
 * Who this contact is, as far as anything can actually be known.
 *
 * The distinction the screen has to carry is between what was *checked* and
 * what was merely *said*. A petname is the user's own, so it is reliable by
 * construction. A persona key is cryptographic and every card is verified
 * against it. An asserted name and an address are neither — they arrived from
 * the other side and nothing here proves they belong to whoever handed the card
 * over.
 *
 * §16.9 makes that split the whole point of the contact model, so the screen
 * groups by it rather than by field type.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ContactProfile(contact: Contact, onBack: () -> Unit, onOpenChat: (Contact) -> Unit) {
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current
    val store = remember { ContactStore(context) }
    var c by remember { mutableStateOf(contact) }
    var petname by remember { mutableStateOf(contact.petname.orEmpty()) }
    var saved by remember { mutableStateOf(false) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(c.displayName()) },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.Filled.ArrowBack, "Back") }
                },
            )
        },
    ) { padding ->
        Column(
            Modifier
                .padding(padding)
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(20.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Avatar(c.displayName(), c.avatar, size = 64)
                Spacer(Modifier.width(14.dp))
                Column {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(c.displayName(), style = MaterialTheme.typography.headlineSmall)
                        c.pronouns?.let { code ->
                            val labels = remember { uniffi.ducat_mobile.pronounOptions() }
                            labels.getOrNull(code - 1)?.let {
                                Spacer(Modifier.width(8.dp))
                                Text(
                                    it,
                                    style = MaterialTheme.typography.labelMedium,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                        }
                    }
                    c.assertedName?.takeIf { it != c.petname }?.let {
                        Text(
                            "calls themselves \"$it\"",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }

            // Their claim about themselves, and labelled as one. Nothing here
            // is verified by anything: DUCAT binds a persona to a key, and
            // binds that key to nothing in the outside world. An email shown
            // beside a persona is what that persona said, which is useful and
            // is not identity.
            val told = listOfNotNull(
                c.email?.let { "Email" to it },
                c.phone?.let { "Phone" to it },
                c.signal?.let { "Signal" to it },
            )
            if (told.isNotEmpty()) {
                Spacer(Modifier.height(18.dp))
                Text("What they shared", style = MaterialTheme.typography.titleSmall)
                Spacer(Modifier.height(2.dp))
                Text(
                    "Their claim, not a check. Nothing ties a DUCAT persona to an " +
                        "email or a number — only to a key.",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.outline,
                )
                Spacer(Modifier.height(8.dp))
                told.forEach { (label, value) ->
                    Row(
                        Modifier.fillMaxWidth()
                            .clickable { clipboard.setText(AnnotatedString(value)) }
                            .padding(vertical = 6.dp),
                    ) {
                        Text(
                            label,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.width(72.dp),
                        )
                        Text(value, style = MaterialTheme.typography.bodySmall)
                    }
                }
            }

            Spacer(Modifier.height(20.dp))
            OutlinedTextField(
                value = petname,
                onValueChange = { if (it.length <= 32) { petname = it; saved = false } },
                label = { Text("Your name for them") },
                supportingText = { Text("Only you see this (§7.5). It is the name shown everywhere.") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(8.dp))
            Button(onClick = {
                store.add(c.copy(petname = petname.trim().ifBlank { null }))
                c = store.all().first { it.personaHex == c.personaHex }
                saved = true
            }) { Text(if (saved) "Saved" else "Save name") }

            Spacer(Modifier.height(24.dp))
            Text("Checked", style = MaterialTheme.typography.titleMedium)
            Text(
                "Verified cryptographically. Every card they hand out is signed by " +
                    "this key and checked against it.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            Field("Persona", c.personaHex, clipboard)

            Spacer(Modifier.height(24.dp))
            Text("Told to you", style = MaterialTheme.typography.titleMedium)
            Text(
                "Supplied by them and not verified by anything. Nothing in DUCAT " +
                    "ties a Monero address or a name to a persona, so check these " +
                    "another way if it matters.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.ducat.changePending,
            )
            Spacer(Modifier.height(8.dp))
            Field("Their name for themselves", c.assertedName ?: "— none given —", clipboard)
            Field(
                "Monero address",
                c.theirAddress ?: "— not shared; they must send a request —",
                clipboard,
            )

            Spacer(Modifier.height(24.dp))
            Text("Where they are reached", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(8.dp))
            Field("Their outbox", c.theirOutbox.ifBlank { "—" }, clipboard)
            Field("Your outbox to them", c.myOutbox.ifBlank { "—" }, clipboard)

            Spacer(Modifier.height(24.dp))
            Button(onClick = { onOpenChat(c) }, modifier = Modifier.fillMaxWidth()) {
                Text("Open chat")
            }

            Spacer(Modifier.height(28.dp))
            // Named rather than silently absent: a profile screen with no
            // mention of these reads as "DUCAT has no notion of them", when the
            // truth is they are next.
            Text("Not built yet", style = MaterialTheme.typography.titleSmall)
            Spacer(Modifier.height(4.dp))
            Text(
                "A picture, pronouns, an email address, and blocking. All optional, " +
                    "all set during setup, and all travelling the same way a name " +
                    "does — asserted by them, never verified by us.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun Field(
    label: String,
    value: String,
    clipboard: androidx.compose.ui.platform.ClipboardManager,
) {
    Column(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
        Text(
            label,
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Row(verticalAlignment = Alignment.CenterVertically) {
            SelectionContainer(Modifier.weight(1f)) {
                Text(
                    value,
                    fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            if (!value.startsWith("—")) {
                TextButton(onClick = { clipboard.setText(AnnotatedString(value)) }) {
                    Text("Copy", style = MaterialTheme.typography.labelSmall)
                }
            }
        }
    }
}
