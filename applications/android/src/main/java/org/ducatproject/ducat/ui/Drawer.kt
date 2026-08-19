package org.ducatproject.ducat.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import kotlinx.coroutines.flow.first
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Languages
import org.ducatproject.ducat.LocaleStore
import org.ducatproject.ducat.NameStore
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.R
import org.ducatproject.ducat.RateStore
import org.ducatproject.ducat.UnitsStore
import org.ducatproject.ducat.WalletStore
import org.ducatproject.ducat.findActivity

/**
 * What the hamburger opens.
 *
 * The long tail lives here so the bottom bar can stay four things and one verb.
 * Each entry is a whole screen rather than a section of one scroll: settings
 * that share a page with a node status readout make both harder to find.
 */
enum class Section(val labelRes: Int) {
    Status(R.string.section_status),
    Profile(R.string.section_profile),
    Contacts(R.string.section_contacts),
    Logs(R.string.section_logs),
    Settings(R.string.section_settings),
    Modes(R.string.section_modes),
}

@Composable
fun DrawerContent(onPick: (Section) -> Unit) {
    val context = LocalContext.current
    val name = remember { NameStore(context).get() }

    ModalDrawerSheet {
        // The header is a way into the profile, not just a label: a missing
        // name is a nudge, and a nudge you cannot act on is a dead end. Tapping
        // "DUCAT" — named or not — opens the profile that the subtitle is about.
        Column(
            Modifier.clickable { onPick(Section.Profile) }.fillMaxWidth().padding(20.dp)
        ) {
            Text("DUCAT", style = MaterialTheme.typography.headlineSmall)
            Spacer(Modifier.height(4.dp))
            Text(
                name ?: stringResource(R.string.drawer_set_name),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        HorizontalDivider()
        Spacer(Modifier.height(8.dp))
        Section.entries.forEach { s ->
            NavigationDrawerItem(
                label = { Text(stringResource(s.labelRes)) },
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
    jumpToBackup: Boolean = false,
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
            Text(
                stringResource(R.string.drawer_speaking,
                    uniffi.ducat_mobile.protocolVersion()),
                style = MaterialTheme.typography.bodySmall,
            )
        }

        Section.Profile -> ProfileSection()

        Section.Contacts -> ContactsAdminSection(onOpenChat)

        Section.Logs -> LogsScreen()

        Section.Settings -> SettingsScreen(themeMode, onThemeChange, jumpToBackup)

        Section.Modes -> ModesScreen()
    }
}

/**
 * The one screen where the app is configured, gathered on one scroll: how it
 * speaks (language), how it looks, how it measures distance and money, what it
 * discloses, and where the keys are backed up.
 */
@Composable
fun SettingsScreen(
    themeMode: ThemeMode,
    onThemeChange: (ThemeMode) -> Unit,
    jumpToBackup: Boolean = false,
) {
    val context = LocalContext.current
    val scroll = rememberScrollState()
    // Sent here by the "back up" nudge rather than by curiosity about
    // languages: backup is the last card on a long screen, and landing at the
    // top of it asks someone who was told to protect their money to go
    // hunting for the way to do it. Scrolling to the end lands on it, because
    // it *is* the end.
    LaunchedEffect(jumpToBackup) {
        if (!jumpToBackup) return@LaunchedEffect
        // maxValue is zero until the column has been measured, and scrolling
        // to zero is the one place this must not land.
        val end = snapshotFlow { scroll.maxValue }.first { it > 0 }
        scroll.animateScrollTo(end)
    }
    Column(
        Modifier.fillMaxSize().verticalScroll(scroll).padding(20.dp)
    ) {
        LanguageSetting()
        Spacer(Modifier.height(24.dp))

        Text(stringResource(R.string.settings_appearance_title),
            style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(8.dp))
        ThemeMode.entries.forEach { m ->
            Row(
                Modifier.fillMaxWidth().padding(vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(selected = themeMode == m, onClick = { onThemeChange(m) })
                Spacer(Modifier.width(8.dp))
                Text(
                    stringResource(
                        when (m) {
                            ThemeMode.System -> R.string.theme_follow_system
                            ThemeMode.Latte -> R.string.theme_latte
                            ThemeMode.Mocha -> R.string.theme_mocha
                        }
                    )
                )
            }
        }
        Spacer(Modifier.height(24.dp))

        DistanceSetting()
        Spacer(Modifier.height(24.dp))

        RateSettings()
        Spacer(Modifier.height(24.dp))

        // §16.16, and the default is the privacy stance: when a message was
        // read is behavioural data, and it leaves this device by choice, not by
        // installing a chat app.
        val cs = remember { ContactStore(context) }
        var receipts by remember { mutableStateOf(cs.readReceipts()) }
        Text(stringResource(R.string.settings_privacy_title),
            style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(8.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Switch(checked = receipts, onCheckedChange = {
                receipts = it; cs.setReadReceipts(it)
            })
            Spacer(Modifier.width(12.dp))
            Column {
                Text(stringResource(R.string.settings_read_receipts))
                Text(
                    stringResource(
                        if (receipts) R.string.settings_read_receipts_on
                        else R.string.settings_read_receipts_off
                    ),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        Spacer(Modifier.height(24.dp))

        val w = remember { WalletStore(context) }
        BackupSettings(
            spendKeyHex = w.spendKeyHex(),
            restoreHeight = w.restoreHeight(),
            personaSecret = PersonaStore(context).secret(),
        )
    }
}

/**
 * The app's language. Every choice is named in its own language, so the row is
 * recognisable to someone who cannot yet read the rest of the app. Choosing one
 * recreates the activity, which is where the new language is applied.
 *
 * A dropdown at the button rather than a dialog: the list opens where the
 * finger already is, scrolls in place, and a tap outside puts it away — no
 * modal ceremony for what is one pick from a list.
 */
@Composable
private fun LanguageSetting() {
    val context = LocalContext.current
    val store = remember { LocaleStore(context) }
    val tag = remember { store.tag() }
    var open by remember { mutableStateOf(false) }

    val current =
        if (tag.isBlank()) stringResource(R.string.settings_language_system)
        else Languages.endonymFor(tag) ?: tag

    fun choose(newTag: String) {
        store.setTag(newTag)
        open = false
        // attachBaseContext runs again on recreate, applying the new locale
        // before any screen is drawn.
        context.findActivity()?.recreate()
    }

    Column {
        Text(stringResource(R.string.settings_language_title),
            style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(8.dp))
        // The menu needs an anchor box so it opens at the button, not the column.
        Box {
            OutlinedButton(onClick = { open = true }) {
                Text(current)
                Spacer(Modifier.width(6.dp))
                Icon(Icons.Filled.ArrowDropDown, null, Modifier.size(18.dp))
            }
            DropdownMenu(
                expanded = open,
                onDismissRequest = { open = false },
                modifier = Modifier.heightIn(max = 420.dp),
            ) {
                LanguageItem(
                    stringResource(R.string.settings_language_system),
                    selected = tag.isBlank(),
                ) { choose("") }
                Languages.SUPPORTED.forEach { l ->
                    LanguageItem(l.endonym, selected = tag == l.tag) { choose(l.tag) }
                }
            }
        }
    }
}

@Composable
private fun LanguageItem(label: String, selected: Boolean, onClick: () -> Unit) {
    DropdownMenuItem(
        text = { Text(label) },
        onClick = onClick,
        // The current choice is marked rather than restated: a check on the row
        // you picked reads at a glance in any script.
        trailingIcon = if (selected) {
            { Icon(Icons.Filled.Check, null, Modifier.size(18.dp)) }
        } else null,
    )
}

/** Kilometres or miles, defaulting to what the device's region uses. */
@Composable
private fun DistanceSetting() {
    val context = LocalContext.current
    val store = remember { UnitsStore(context) }
    var sys by remember { mutableStateOf(store.system()) }

    val options = listOf(
        UnitsStore.SYSTEM to stringResource(R.string.distance_follow_system),
        UnitsStore.METRIC to stringResource(R.string.distance_kilometres),
        UnitsStore.IMPERIAL to stringResource(R.string.distance_miles),
    )
    Column {
        Text(stringResource(R.string.settings_distance_title),
            style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(8.dp))
        options.forEach { (value, label) ->
            Row(
                Modifier.fillMaxWidth().padding(vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(
                    selected = sys == value,
                    onClick = { sys = value; store.setSystem(value); ContactStore.bump() },
                )
                Spacer(Modifier.width(8.dp))
                Text(label)
            }
        }
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
        Text(stringResource(R.string.settings_prices_title),
            style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(8.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Switch(checked = on, onCheckedChange = { on = it; store.setEnabled(it) })
            Spacer(Modifier.width(12.dp))
            Text(stringResource(R.string.prices_show_worth))
        }
        if (on) {
            Spacer(Modifier.height(8.dp))
            var open by remember { mutableStateOf(false) }
            // The same shape as the language picker above: a dropdown at the
            // button, scrolling in place, dismissed by tapping elsewhere.
            Box {
                OutlinedButton(onClick = { open = true }) {
                    Text(cur)
                    Spacer(Modifier.width(6.dp))
                    Icon(Icons.Filled.ArrowDropDown, null, Modifier.size(18.dp))
                }
                DropdownMenu(
                    expanded = open,
                    onDismissRequest = { open = false },
                    modifier = Modifier.heightIn(max = 420.dp),
                ) {
                    RateStore.SUPPORTED.forEach { c ->
                        DropdownMenuItem(
                            text = {
                                Row(verticalAlignment = Alignment.CenterVertically) {
                                    Text(c)
                                    if (c == store.deviceCurrency()) {
                                        Spacer(Modifier.width(8.dp))
                                        Text(
                                            stringResource(R.string.prices_this_phone),
                                            style = MaterialTheme.typography.labelSmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                    }
                                }
                            },
                            onClick = { cur = c; store.setCurrency(c); open = false },
                            trailingIcon = if (cur == c) {
                                { Icon(Icons.Filled.Check, null, Modifier.size(18.dp)) }
                            } else null,
                        )
                    }
                }
            }
            Spacer(Modifier.height(8.dp))
            val note = stringResource(R.string.prices_source_note)
            val full = if (store.source().isNotEmpty())
                note + " " + stringResource(R.string.prices_last_from, store.source())
            else note
            Text(
                full,
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
            Text(stringResource(R.string.drawer_persona_title),
                style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(4.dp))
            Text(
                stringResource(R.string.drawer_persona_body),
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
        Text(stringResource(R.string.drawer_being_paid_title),
            style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(8.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Switch(checked = on, onCheckedChange = { on = it; store.setPublishAddress(it) })
            Spacer(Modifier.width(12.dp))
            Column {
                Text(stringResource(R.string.drawer_publish_switch))
                Text(
                    stringResource(R.string.drawer_publish_off_note),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        if (on) {
            Spacer(Modifier.height(8.dp))
            Text(
                stringResource(R.string.drawer_publish_on_warning),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.ducat.changePending,
            )
        }
        Spacer(Modifier.height(6.dp))
        Text(
            stringResource(R.string.drawer_publish_scope_note),
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
    // Keyed on the store's version like every other list: a rename, a new
    // claim, or a forget made anywhere else must show here without leaving
    // and reopening the section.
    val version by ContactStore.changes.collectAsState()
    var contacts by remember(version) { mutableStateOf(store.all()) }
    var confirm by remember { mutableStateOf<Contact?>(null) }
    var profileOf by remember { mutableStateOf<Contact?>(null) }

    profileOf?.let { p ->
        // A full-screen overlay, not a nested screen: nesting put a second
        // top bar under the section's own and left the system back button
        // wired to neither — it exited to Home past both arrows. A dialog
        // carries exactly one header, covers the bar beneath instead of
        // stacking, and hands its dismissal to the back button for free.
        androidx.compose.ui.window.Dialog(
            onDismissRequest = { profileOf = null; contacts = store.all() },
            properties = androidx.compose.ui.window.DialogProperties(
                usePlatformDefaultWidth = false,
            ),
        ) {
            Surface(
                Modifier.fillMaxSize(),
                color = MaterialTheme.colorScheme.background,
            ) {
                ContactProfile(
                    contact = p,
                    onBack = { profileOf = null; contacts = store.all() },
                    onOpenChat = { profileOf = null; onOpenChat(it) },
                )
            }
        }
    }

    if (contacts.isEmpty()) {
        Box(Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
            Text(
                stringResource(R.string.drawer_no_contacts),
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
                            Icon(Icons.Filled.ChatBubbleOutline,
                                stringResource(R.string.drawer_open_chat))
                        }
                        IconButton(onClick = { confirm = c }) {
                            Icon(
                                Icons.Filled.DeleteOutline,
                                stringResource(R.string.drawer_delete_contact),
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
            title = { Text(stringResource(R.string.drawer_forget_title, c.displayName())) },
            text = { Text(stringResource(R.string.drawer_forget_body)) },
            confirmButton = {
                TextButton(onClick = {
                    store.forget(c.personaHex)
                    contacts = store.all()
                    confirm = null
                }) {
                    Text(stringResource(R.string.drawer_forget_confirm),
                        color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = {
                TextButton(onClick = { confirm = null }) {
                    Text(stringResource(R.string.common_cancel))
                }
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

    // A picker, not toggles: a mode is what this device *is* right now, and a
    // thing can only be one thing. Choosing one hands the whole app to that
    // job; Personal is the wallet-and-chat app this list starts from.
    fun pick(m: org.ducatproject.ducat.Mode) {
        current = m
        modes.set(m)
        org.ducatproject.ducat.DucatLog.i("Mode", "switched to $m")
    }

    val options = listOf(
        Triple(
            org.ducatproject.ducat.Mode.None,
            stringResource(R.string.mode_personal),
            stringResource(R.string.mode_personal_desc),
        ),
        Triple(
            org.ducatproject.ducat.Mode.Pos,
            stringResource(R.string.mode_pos),
            stringResource(R.string.mode_pos_desc),
        ),
        Triple(
            org.ducatproject.ducat.Mode.BarTab,
            stringResource(R.string.mode_bartab),
            stringResource(R.string.mode_bartab_desc),
        ),
        Triple(
            org.ducatproject.ducat.Mode.Taxi,
            stringResource(R.string.mode_taxi),
            stringResource(R.string.mode_taxi_desc),
        ),
        Triple(
            org.ducatproject.ducat.Mode.Donate,
            stringResource(R.string.mode_donate),
            stringResource(R.string.mode_donate_desc),
        ),
        Triple(
            org.ducatproject.ducat.Mode.Renting,
            stringResource(R.string.mode_renting),
            stringResource(R.string.mode_renting_desc),
        ),
        Triple(
            org.ducatproject.ducat.Mode.Kiosk,
            stringResource(R.string.kiosk_mode_title),
            stringResource(R.string.kiosk_mode_body),
        ),
    )

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp)) {
        Text(
            stringResource(R.string.modes_intro),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(16.dp))

        Card(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(vertical = 4.dp)) {
                options.forEachIndexed { i, (mode, title, detail) ->
                    if (i > 0) HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                    Row(
                        Modifier.fillMaxWidth()
                            .clickable { pick(mode) }
                            .padding(horizontal = 16.dp, vertical = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(selected = current == mode, onClick = { pick(mode) })
                        Spacer(Modifier.width(8.dp))
                        Column(Modifier.weight(1f)) {
                            Text(title, style = MaterialTheme.typography.titleMedium)
                            Text(
                                detail,
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
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
