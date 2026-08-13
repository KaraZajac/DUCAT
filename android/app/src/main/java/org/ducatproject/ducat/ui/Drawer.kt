package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.NameStore
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.WalletStore

/**
 * What the hamburger opens.
 *
 * The long tail lives here so the bottom bar can stay four things and one verb.
 * Each entry is a whole screen rather than a section of one scroll: settings
 * that share a page with a node status readout make both harder to find.
 */
enum class Section(val label: String) {
    Status("Status"),
    Profile("Profile"),
    Contacts("Contacts"),
    Settings("Settings"),
    Modes("Operating modes"),
}

@Composable
fun DrawerContent(onPick: (Section) -> Unit) {
    val context = LocalContext.current
    val name = remember { NameStore(context).get() }

    ModalDrawerSheet {
        Column(Modifier.padding(20.dp)) {
            Text("DUCAT", style = MaterialTheme.typography.headlineSmall)
            Spacer(Modifier.height(4.dp))
            Text(
                name ?: "no profile name set",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        HorizontalDivider()
        Spacer(Modifier.height(8.dp))
        Section.entries.forEach { s ->
            NavigationDrawerItem(
                label = { Text(s.label) },
                icon = { Icon(iconFor(s), null) },
                selected = false,
                // Modes is listed and disabled rather than hidden: §8.8's
                // provider roles are real and unbuilt, and a menu that quietly
                // omits them tells a user less than one that says "not yet".
                onClick = { if (s != Section.Modes) onPick(s) },
                badge = if (s == Section.Modes) {
                    { Text("later", style = MaterialTheme.typography.labelSmall) }
                } else null,
                modifier = Modifier.padding(horizontal = 12.dp),
            )
        }
    }
}

private fun iconFor(s: Section) = when (s) {
    Section.Status -> Icons.Filled.Lan
    Section.Profile -> Icons.Filled.Person
    Section.Contacts -> Icons.Filled.People
    Section.Settings -> Icons.Filled.Settings
    Section.Modes -> Icons.Filled.Tune
}

@Composable
fun SectionScreen(
    section: Section,
    themeMode: ThemeMode,
    onThemeChange: (ThemeMode) -> Unit,
    onOpenChat: (Contact) -> Unit,
) {
    val context = LocalContext.current
    when (section) {
        Section.Status -> Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp)
        ) {
            NetworkPanel(storageDir = context.filesDir.absolutePath + "/veilid")
            Spacer(Modifier.height(16.dp))
            MoneroPanel()
            Spacer(Modifier.height(16.dp))
            Text("speaking ${uniffi.ducat_mobile.protocolVersion()}",
                 style = MaterialTheme.typography.bodySmall)
        }

        Section.Profile -> ProfileSection()

        Section.Contacts -> ContactsAdminSection(onOpenChat)

        Section.Settings -> Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp)
        ) {
            Text("Appearance", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(8.dp))
            ThemeMode.entries.forEach { m ->
                Row(
                    Modifier.fillMaxWidth().padding(vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RadioButton(selected = themeMode == m, onClick = { onThemeChange(m) })
                    Spacer(Modifier.width(8.dp))
                    Text(
                        when (m) {
                            ThemeMode.System -> "Follow system"
                            ThemeMode.Latte -> "Latte (light)"
                            ThemeMode.Mocha -> "Mocha (dark)"
                        }
                    )
                }
            }
            Spacer(Modifier.height(20.dp))
            val w = remember { WalletStore(context) }
            BackupSettings(
                spendKeyHex = w.spendKeyHex(),
                restoreHeight = w.restoreHeight(),
                personaSecret = PersonaStore(context).secret(),
            )
        }

        Section.Modes -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Text("Not built yet.", style = MaterialTheme.typography.bodyLarge)
        }
    }
}

/** Your own name and persona — the identity other people save you under. */
@Composable
private fun ProfileSection() {
    val context = LocalContext.current
    val store = remember { NameStore(context) }
    var name by remember { mutableStateOf(store.get() ?: "") }
    var saved by remember { mutableStateOf(false) }
    val persona = remember { PersonaStore(context).personaHex() }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp)) {
        Text("Your name", style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(4.dp))
        Text(
            "Shown on cards you hand out. It is a suggestion, not an identity — " +
                "whoever adds you can rename you, and that name is the one they see.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(12.dp))
        OutlinedTextField(
            value = name,
            onValueChange = { if (it.length <= 32) { name = it; saved = false } },
            label = { Text("Name") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(12.dp))
        Button(onClick = { store.put(name.trim()); name = name.trim(); saved = true }) {
            Text(if (saved) "Saved" else "Save")
        }

        Spacer(Modifier.height(28.dp))
        Text("Your persona", style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(4.dp))
        Text(
            "The key your cards are signed with. Nobody can find you by it — " +
                "it is what someone checks a card against once they have one.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(8.dp))
        SelectionContainerText(persona)
    }
}

/** Every contact, with the two different kinds of removal spelled out. */
@Composable
private fun ContactsAdminSection(onOpenChat: (Contact) -> Unit) {
    val context = LocalContext.current
    val store = remember { ContactStore(context) }
    var contacts by remember { mutableStateOf(store.all()) }
    var confirm by remember { mutableStateOf<Contact?>(null) }

    if (contacts.isEmpty()) {
        Box(Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
            Text(
                "No contacts yet. Add one from the Chat tab.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        return
    }

    LazyColumn(Modifier.fillMaxSize()) {
        items(contacts, key = { it.personaHex }) { c ->
            ListItem(
                headlineContent = { Text(c.displayName()) },
                supportingContent = {
                    Text(
                        c.personaHex.take(24) + "…",
                        fontFamily = FontFamily.Monospace,
                        style = MaterialTheme.typography.bodySmall,
                    )
                },
                trailingContent = {
                    Row {
                        IconButton(onClick = { onOpenChat(c) }) {
                            Icon(Icons.Filled.ChatBubbleOutline, "Open chat")
                        }
                        IconButton(onClick = { confirm = c }) {
                            Icon(
                                Icons.Filled.DeleteOutline,
                                "Delete contact",
                                tint = MaterialTheme.colorScheme.error,
                            )
                        }
                    }
                },
            )
            HorizontalDivider()
        }
    }

    confirm?.let { c ->
        AlertDialog(
            onDismissRequest = { confirm = null },
            title = { Text("Forget ${c.displayName()}?") },
            text = {
                Text(
                    "This deletes the contact and every message from them. They " +
                        "cannot be recovered, and reaching them again needs a new " +
                        "card."
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    store.forget(c.personaHex)
                    contacts = store.all()
                    confirm = null
                }) { Text("Forget", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = {
                TextButton(onClick = { confirm = null }) { Text("Cancel") }
            },
        )
    }
}

@Composable
private fun SelectionContainerText(text: String) {
    androidx.compose.foundation.text.selection.SelectionContainer {
        Text(text, fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.bodySmall)
    }
}
