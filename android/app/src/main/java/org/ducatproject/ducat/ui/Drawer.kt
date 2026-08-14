package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.foundation.clickable
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
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
import org.ducatproject.ducat.RateStore
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
    Logs("Logs"),
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
                onClick = { onPick(s) },
                modifier = Modifier.padding(horizontal = 12.dp),
            )
        }
    }
}

private fun iconFor(s: Section) = when (s) {
    Section.Status -> Icons.Filled.Lan
    Section.Profile -> Icons.Filled.Person
    Section.Contacts -> Icons.Filled.People
    Section.Logs -> Icons.Filled.Description
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

        Section.Logs -> LogsScreen()

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

        Section.Modes -> ModesScreen()
    }
}

/**
 * Whether to look up a price at all, and in what currency.
 *
 * Off means no request is made, not a quieter one. Asking tells whoever answers
 * that this device cares about Monero's price — smaller than what the wallet
 * already tells a public node, but not nothing, and not something to do on a
 * user's behalf without a way to decline.
 */
@Composable
private fun RateSettings() {
    val context = LocalContext.current
    val store = remember { RateStore(context) }
    var on by remember { mutableStateOf(store.enabled()) }
    var cur by remember { mutableStateOf(store.currency()) }

    Column {
        Text("Prices", style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(8.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Switch(checked = on, onCheckedChange = { on = it; store.setEnabled(it) })
            Spacer(Modifier.width(12.dp))
            Text("Show what a balance is worth")
        }
        if (on) {
            Spacer(Modifier.height(8.dp))
            var picking by remember { mutableStateOf(false) }
            OutlinedButton(onClick = { picking = true }) {
                Text(cur)
                Spacer(Modifier.width(6.dp))
                Icon(Icons.Filled.ArrowDropDown, null, Modifier.size(18.dp))
            }
            if (picking) {
                AlertDialog(
                    onDismissRequest = { picking = false },
                    title = { Text("Currency") },
                    text = {
                        LazyColumn(Modifier.heightIn(max = 360.dp)) {
                            items(RateStore.SUPPORTED) { c ->
                                Row(
                                    Modifier.fillMaxWidth()
                                        .clickable { cur = c; store.setCurrency(c); picking = false }
                                        .padding(vertical = 10.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    RadioButton(
                                        selected = cur == c,
                                        onClick = { cur = c; store.setCurrency(c); picking = false },
                                    )
                                    Spacer(Modifier.width(8.dp))
                                    Text(c)
                                    if (c == store.deviceCurrency()) {
                                        Spacer(Modifier.width(8.dp))
                                        Text(
                                            "this phone",
                                            style = MaterialTheme.typography.labelSmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                    }
                                }
                            }
                        }
                    },
                    confirmButton = { TextButton(onClick = { picking = false }) { Text("Done") } },
                )
            }
            Spacer(Modifier.height(8.dp))
            Text(
                "Checked at most twice an hour, from CoinGecko or Kraken. Turning " +
                    "this off stops the request entirely." +
                    (if (store.source().isNotEmpty()) " Last from ${store.source()}." else ""),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** Your own name and persona — the identity other people save you under. */
@Composable
private fun ProfileSection() {
    val context = LocalContext.current
    val persona = remember { PersonaStore(context).personaHex() }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        // The whole profile, including the name — it used to live here as a
        // lone text field, and splitting "what people see of you" across two
        // screens is how a name and a picture end up disagreeing.
        MyProfileEditor()

        Column(Modifier.padding(20.dp)) {
            PublishAddressSetting()

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
}

/**
 * Whether contacts may pay without asking first.
 *
 * Off by default, and the cost is stated once and plainly rather than buried:
 * §16.12 makes this a choice about the user's own linkability, and choosing for
 * them — in either direction — is not a choice they made.
 */
@Composable
private fun PublishAddressSetting() {
    val context = LocalContext.current
    val store = remember { ContactStore(context) }
    var on by remember { mutableStateOf(store.publishAddress()) }

    Column {
        Text("Being paid", style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(8.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Switch(checked = on, onCheckedChange = { on = it; store.setPublishAddress(it) })
            Spacer(Modifier.width(12.dp))
            Column {
                Text("Let contacts pay me directly")
                Text(
                    "Without this, someone paying you has to wait for a request.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        if (on) {
            Spacer(Modifier.height(8.dp))
            Text(
                "Your address goes to each contact once, and gets reused. Anyone " +
                    "who can see the chain can tell that the same person was paid " +
                    "each time — including people who only ever paid you once. " +
                    "Requests avoid that by carrying a fresh address each time.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.ducat.changePending,
            )
        }
        Spacer(Modifier.height(6.dp))
        Text(
            "Only contacts added after this is on will have it.",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.outline,
        )
    }
}

/** Every contact, with the two different kinds of removal spelled out. */
@Composable
private fun ContactsAdminSection(onOpenChat: (Contact) -> Unit) {
    val context = LocalContext.current
    val store = remember { ContactStore(context) }
    var contacts by remember { mutableStateOf(store.all()) }
    var confirm by remember { mutableStateOf<Contact?>(null) }
    var profileOf by remember { mutableStateOf<Contact?>(null) }

    profileOf?.let { p ->
        ContactProfile(
            contact = p,
            onBack = { profileOf = null; contacts = store.all() },
            onOpenChat = { profileOf = null; onOpenChat(it) },
        )
        return
    }

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
                modifier = Modifier.clickable { profileOf = c },
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


/**
 * What this device is for (§15).
 *
 * A toggle, not a navigation item: a mode is a standing state, and the proof it
 * is on should be the app itself — switch the till on and Home *is* the till
 * until it is switched off. The ones not built yet say so and cannot be turned
 * on, which beats a menu of five entries where four disappoint.
 */
@Composable
fun ModesScreen() {
    val context = LocalContext.current
    val modes = remember { org.ducatproject.ducat.ModeStore(context) }
    var current by remember { mutableStateOf(modes.current()) }

    // One at a time, enforced here: switching one on switches the rest off. A
    // device that is simultaneously a till and a taxi meter has two ideas
    // about what an arriving payment means.
    fun pick(m: org.ducatproject.ducat.Mode, on: Boolean) {
        current = if (on) m else org.ducatproject.ducat.Mode.None
        modes.set(current)
        org.ducatproject.ducat.DucatLog.i("Mode", "switched to $current")
    }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp)) {
        Text(
            "A mode changes what this device leads with. Switch one on and the " +
                "Home tab becomes that job until you switch it off. One at a time.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(16.dp))

        Card(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) {
                ModeRow(
                    "Point of sale",
                    "Ring up a sale, show one code — the bill and the receipt " +
                        "travel the conversation it opens.",
                    current == org.ducatproject.ducat.Mode.Pos,
                ) { pick(org.ducatproject.ducat.Mode.Pos, it) }
                HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                ModeRow(
                    "Bar tab",
                    "A running tab per customer — scan once, add all night, one " +
                        "bill at close. They can pay after they leave.",
                    current == org.ducatproject.ducat.Mode.BarTab,
                ) { pick(org.ducatproject.ducat.Mode.BarTab, it) }
                HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                ModeRow(
                    "Taxi",
                    "The rate goes in writing when the meter starts; the bill " +
                        "shows the minutes.",
                    current == org.ducatproject.ducat.Mode.Taxi,
                ) { pick(org.ducatproject.ducat.Mode.Taxi, it) }
                HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                ModeRow(
                    "Donate",
                    "A standing code any Monero wallet can give to. No app needed " +
                        "on their side.",
                    current == org.ducatproject.ducat.Mode.Donate,
                ) { pick(org.ducatproject.ducat.Mode.Donate, it) }
            }
        }
    }
}

@Composable
private fun ModeRow(
    title: String,
    detail: String,
    checked: Boolean,
    onChange: (Boolean) -> Unit,
) {
    Row(
        Modifier.fillMaxWidth().padding(vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.titleMedium)
            Text(
                detail,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Spacer(Modifier.width(12.dp))
        Switch(checked = checked, onCheckedChange = onChange)
    }
}
