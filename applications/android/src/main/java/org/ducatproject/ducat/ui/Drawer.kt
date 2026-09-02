package org.ducatproject.ducat.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.MenuBook
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.ui.graphics.asImageBitmap
import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.SafeImage
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.draw.clip
import androidx.compose.runtime.*
import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
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
import org.ducatproject.ducat.saidWhy

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
    Library(R.string.section_library),
    Sites(R.string.section_sites),
    Selling(R.string.section_selling),
    // Routable but not listed: the Press mode's second tab and the
    // market's "list yours" open it directly, and the Press room is
    // where publishing lives now — a drawer row would be a second door
    // to the same desk.
    Publishing(R.string.section_publishing),
    Logs(R.string.section_logs),
    Settings(R.string.section_settings),
    Modes(R.string.section_modes),
}

@Composable
fun DrawerContent(onPick: (Section) -> Unit) {
    val context = LocalContext.current
    // Keyed on the store's own version, like every other read of it in the
    // app. `remember {}` with no key reads the name once and keeps it for the
    // life of the composition, so somebody who set or changed their name — in
    // the profile this very header opens, one tap away — came back to a
    // drawer still offering to set it.
    val version by ContactStore.changes.collectAsState()
    val name = remember(version) { NameStore(context).get() }

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
        PersonaSwitcher(Modifier.padding(horizontal = 16.dp).padding(bottom = 12.dp))
        HorizontalDivider()
        Spacer(Modifier.height(8.dp))
        Section.entries.filter { it != Section.Publishing }.forEach { s ->
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
    Section.Library -> Icons.Filled.LocalLibrary
    Section.Sites -> Icons.Filled.Public
    Section.Selling -> Icons.Filled.Storefront
    Section.Publishing -> Icons.AutoMirrored.Filled.MenuBook
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

        Section.Library -> LibrarySection()

        Section.Sites -> SitesSection()
        Section.Selling -> SellingSection()
        Section.Publishing -> PublishingSection()

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
            // The whole row, not the dot. Settings had these two groups
            // answering only to a tap on the eleven-dp circle itself, while
            // the operating-mode list two hundred lines down and the chat's
            // retention picker both take the row — so the same control
            // behaved differently depending on which screen you met it on,
            // and the one people meet first was the fussy one.
            //
            // `selectable` rather than `clickable`, with the button's own
            // onClick given up: it puts the radio's role on the row, so a
            // screen reader announces one control instead of a button beside
            // a label.
        ThemeMode.entries.forEach { m ->
            Row(
                Modifier.fillMaxWidth()
                    .selectable(
                        selected = themeMode == m,
                        onClick = { onThemeChange(m) },
                        role = androidx.compose.ui.semantics.Role.RadioButton,
                    )
                    .padding(vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(selected = themeMode == m, onClick = null)
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

        FareRegion()
        Spacer(Modifier.height(24.dp))
        TaxSetting()
        Spacer(Modifier.height(24.dp))
        RecurringSetting()

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
            // The whole row — see the theme group above.
            Row(
                Modifier.fillMaxWidth()
                    .selectable(
                        selected = sys == value,
                        onClick = { sys = value; store.setSystem(value); ContactStore.bump() },
                        role = androidx.compose.ui.semantics.Role.RadioButton,
                    )
                    .padding(vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(selected = sys == value, onClick = null)
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
/**
 * Where fares are priced for.
 *
 * §15.12's suggestion is built from what a taxi charges locally, and "locally"
 * is a country — the table has a hundred and one of them. The phone's own
 * region is right for nearly everybody, because drivers work where they live;
 * it is wrong for the tourist whose phone still says home, and for anybody
 * working across a border, and until now there was no way for either of them
 * to say so.
 *
 * The names come from the platform, so a hundred countries cost no strings
 * and each reader sees them in their own language.
 */
@Composable
private fun FareRegion() {
    val context = LocalContext.current
    var iso by remember { mutableStateOf(org.ducatproject.ducat.Fare.country(context)) }
    var open by remember { mutableStateOf(false) }
    val phone = remember { java.util.Locale.getDefault().country }
    fun name(code: String): String =
        java.util.Locale("", code).getDisplayCountry(java.util.Locale.getDefault())
            .ifBlank { code }

    Column {
        Text(
            stringResource(R.string.settings_fares_title),
            style = MaterialTheme.typography.titleMedium,
        )
        Spacer(Modifier.height(4.dp))
        Text(
            stringResource(R.string.settings_fares_note),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(8.dp))
        Box {
            OutlinedButton(onClick = { open = true }) {
                Text(name(iso))
                Spacer(Modifier.width(6.dp))
                Icon(Icons.Filled.ArrowDropDown, null, Modifier.size(18.dp))
            }
            DropdownMenu(
                expanded = open,
                onDismissRequest = { open = false },
                modifier = Modifier.heightIn(max = 420.dp),
            ) {
                // Only the surveyed ones: offering a country the table cannot
                // price would be a setting that changes nothing.
                java.util.Locale.getISOCountries()
                    .filter { org.ducatproject.ducat.FareRates.known(it) }
                    .sortedBy { name(it).lowercase() }
                    .forEach { c ->
                        DropdownMenuItem(
                            text = {
                                Row(verticalAlignment = Alignment.CenterVertically) {
                                    Text(name(c))
                                    if (c == phone) {
                                        Spacer(Modifier.width(8.dp))
                                        Text(
                                            stringResource(R.string.prices_this_phone),
                                            style = MaterialTheme.typography.labelSmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                    }
                                }
                            },
                            onClick = {
                                iso = c
                                org.ducatproject.ducat.Fare.setCountry(context, c)
                                open = false
                            },
                        )
                    }
            }
        }
        if (!org.ducatproject.ducat.FareRates.known(iso)) {
            Spacer(Modifier.height(6.dp))
            Text(
                stringResource(R.string.settings_fares_unsurveyed),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun RateSettings() {
    val context = LocalContext.current
    val store = remember { RateStore(context) }
    var on by remember { mutableStateOf(store.enabled()) }
    var cur by remember { mutableStateOf(store.currency()) }
    // A currency change that would strand priced items waits for a yes.
    var pendingCurrency by remember { mutableStateOf<String?>(null) }
    var menuWarnCount by remember { mutableStateOf(0) }

    pendingCurrency?.takeIf { menuWarnCount > 0 }?.let { next ->
        AlertDialog(
            onDismissRequest = { pendingCurrency = null; menuWarnCount = 0 },
            title = { Text(stringResource(R.string.prices_menu_warn_title, cur)) },
            text = { Text(stringResource(R.string.prices_menu_warn_body, cur, next)) },
            confirmButton = {
                TextButton(onClick = {
                    cur = next; store.setCurrency(next)
                    pendingCurrency = null; menuWarnCount = 0
                }) { Text(stringResource(R.string.prices_menu_warn_go)) }
            },
            dismissButton = {
                TextButton(onClick = { pendingCurrency = null; menuWarnCount = 0 }) {
                    Text(stringResource(R.string.rent_cancel))
                }
            },
        )
    }

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
                            onClick = {
                                // A catalogue price is stored as text plus the
                                // currency it was typed in, and there is only
                                // ever a rate for the one currency the store is
                                // set to — so items priced in the old one stop
                                // being sellable the moment this changes. That
                                // is the right behaviour (converting a shop's
                                // prices behind its back would be worse) but it
                                // used to happen in silence, and the first sign
                                // was a till full of dead buttons with a queue
                                // in front of it. Said here, before, with a
                                // count, so it is a decision rather than a
                                // discovery.
                                val orphaned = org.ducatproject.ducat.Catalogue
                                    .live(context)
                                    .count { it.currency.isNotBlank() && it.currency != c }
                                if (orphaned == 0) {
                                    cur = c
                                    store.setCurrency(c)
                                } else {
                                    pendingCurrency = c
                                    menuWarnCount = orphaned
                                }
                                open = false
                            },
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
    val version by ContactStore.changes.collectAsState()
    val personas = remember { PersonaStore(context) }
    val roster = remember(version) { personas.all() }
    val worn = remember(version) { personas.worn() }
    val modes = remember { org.ducatproject.ducat.ModeStore(context) }

    // Which profile is on the desk. Editing is not wearing: the hat is
    // switched in the drawer header, at the doorway; this only chooses
    // whose presentation the fields below belong to.
    var picked by rememberSaveable { mutableStateOf<String?>(null) }
    val editing = picked?.takeIf { h -> roster.any { it.hex == h } } ?: worn
    var adding by remember { mutableStateOf(false) }
    var renaming by remember { mutableStateOf(false) }
    var newLabel by remember { mutableStateOf("") }

    val modeNames = mapOf(
        org.ducatproject.ducat.Mode.None to stringResource(R.string.mode_personal),
        org.ducatproject.ducat.Mode.Pos to stringResource(R.string.mode_pos),
        org.ducatproject.ducat.Mode.BarTab to stringResource(R.string.mode_bartab),
        org.ducatproject.ducat.Mode.Taxi to stringResource(R.string.mode_taxi),
        org.ducatproject.ducat.Mode.Donate to stringResource(R.string.mode_donate),
        org.ducatproject.ducat.Mode.Renting to stringResource(R.string.mode_renting),
        org.ducatproject.ducat.Mode.Marketplace to stringResource(R.string.mode_marketplace),
        org.ducatproject.ducat.Mode.HireHelp to stringResource(R.string.mode_hire_help),
        org.ducatproject.ducat.Mode.Press to stringResource(R.string.mode_press),
    )
    val bindings = remember(version) {
        roster.associate { p ->
            p.hex to modeNames.keys.filter { m -> modes.boundPersona(m) == p.hex }
        }
    }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Column(Modifier.padding(horizontal = 20.dp).padding(top = 16.dp)) {
            Text(
                stringResource(R.string.personas_body),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(12.dp))

            roster.forEach { pr ->
                val active = pr.hex == editing
                // The card carries the profile's public face — the avatar
                // and name contacts see — not the compartment label's
                // initial. A card saying "P" above an editor saying "S"
                // read as two different people.
                val mp = remember(version, pr.hex) { MyProfile(context, pr.hex) }
                val pic = remember(version, pr.hex) { mp.avatar() }
                val publicName = remember(version, pr.hex) { mp.name() }
                Card(
                    Modifier.fillMaxWidth().padding(bottom = 8.dp),
                    colors = CardDefaults.cardColors(
                        containerColor = if (active) {
                            MaterialTheme.colorScheme.primaryContainer
                        } else {
                            MaterialTheme.colorScheme.surfaceContainerHigh
                        },
                    ),
                ) {
                    Row(
                        Modifier.fillMaxWidth()
                            .clickable { picked = pr.hex; renaming = false }
                            .padding(horizontal = 14.dp, vertical = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Box(
                            Modifier.size(40.dp).clip(CircleShape).background(
                                if (pr.color != 0) {
                                    androidx.compose.ui.graphics.Color(pr.color)
                                } else {
                                    MaterialTheme.colorScheme.primary
                                },
                            ),
                            contentAlignment = Alignment.Center,
                        ) {
                            val bmp = remember(pic) {
                                pic?.let { SafeImage.fromBytes(it, SafeImage.AVATAR_PIXELS) }
                            }
                            if (bmp != null) {
                                androidx.compose.foundation.Image(
                                    bmp.asImageBitmap(), null,
                                    Modifier.fillMaxSize(),
                                    contentScale = androidx.compose.ui.layout.ContentScale.Crop,
                                )
                            } else {
                                Text(
                                    (publicName ?: personaLabel(pr)).take(1).uppercase(),
                                    style = MaterialTheme.typography.titleMedium,
                                    color = androidx.compose.ui.graphics.Color.White,
                                )
                            }
                        }
                        Spacer(Modifier.width(12.dp))
                        Column(Modifier.weight(1f)) {
                            Text(personaLabel(pr), style = MaterialTheme.typography.titleMedium)
                            val sub = buildList {
                                if (pr.hex == personas.personaHex()) {
                                    add(stringResource(R.string.profiles_personal_sub))
                                }
                                if (publicName != null && publicName != personaLabel(pr)) {
                                    add(stringResource(R.string.profiles_appears_as, publicName))
                                }
                                val bound = bindings[pr.hex].orEmpty()
                                if (bound.isNotEmpty()) {
                                    add(
                                        stringResource(
                                            R.string.profiles_answers_for,
                                            bound.mapNotNull { modeNames[it] }.joinToString(", "),
                                        ),
                                    )
                                }
                            }
                            if (sub.isNotEmpty()) {
                                Text(
                                    sub.joinToString(" · "),
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                        }
                        if (pr.hex == worn) {
                            Spacer(Modifier.width(8.dp))
                            Text(
                                stringResource(R.string.personas_worn),
                                style = MaterialTheme.typography.labelMedium,
                                color = MaterialTheme.colorScheme.primary,
                            )
                        }
                        if (active) {
                            // A bare icon, not an IconButton: the 48 dp
                            // touch target of the latter squeezed the
                            // subtitle into a wrap on the wearing card.
                            Icon(
                                Icons.Filled.Edit,
                                contentDescription =
                                    stringResource(R.string.profiles_rename),
                                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                modifier = Modifier
                                    .padding(start = 8.dp)
                                    .size(18.dp)
                                    .clickable {
                                        newLabel = pr.name
                                        renaming = !renaming
                                    },
                            )
                        }
                    }
                }
                if (active && renaming) {
                    Row(
                        Modifier.padding(bottom = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        OutlinedTextField(
                            value = newLabel,
                            onValueChange = { if (it.length <= 24) newLabel = it },
                            label = { Text(stringResource(R.string.personas_name_label)) },
                            singleLine = true,
                            modifier = Modifier.weight(1f),
                        )
                        TextButton(onClick = {
                            if (newLabel.isNotBlank() || pr.name.isBlank()) {
                                personas.rename(pr.hex, newLabel.trim())
                            }
                            renaming = false
                            ContactStore.bump()
                        }) { Text(stringResource(R.string.personas_save)) }
                    }
                }
            }
            if (roster.size < PersonaStore.MAX_PERSONAS) {
                TextButton(onClick = { adding = true }) {
                    Text(stringResource(R.string.personas_add))
                }
            } else {
                Text(
                    stringResource(R.string.personas_cap),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.outline,
                )
            }
        }

        Spacer(Modifier.height(8.dp))
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)

        // Whose desk the fields below belong to — with the label editable
        // in place, because the card list is where the label lives.
        val editingPersona = roster.firstOrNull { it.hex == editing }
        if (editingPersona != null) {
            val editingPublic = remember(version, editing) {
                MyProfile(context, editing).name()
            }
            Text(
                stringResource(
                    R.string.profiles_editing_note,
                    editingPublic ?: personaLabel(editingPersona),
                ),
                style = MaterialTheme.typography.titleSmall,
                modifier = Modifier.padding(horizontal = 20.dp).padding(top = 16.dp),
            )
        }

        MyProfileEditor(personaHex = editing)

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
            SelectionContainerText(editing)
        }
    }

    if (adding) {
        NewProfileDialog(onDone = { created ->
            adding = false
            if (created != null) picked = created
        })
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
internal fun ContactsAdminPreview() = ContactsAdminSection(onOpenChat = {})

@Composable
@OptIn(ExperimentalFoundationApi::class)
private fun ContactsAdminSection(onOpenChat: (Contact) -> Unit) {
    val context = LocalContext.current
    val store = remember { ContactStore(context) }
    // Keyed on the store's version like every other list: a rename, a new
    // claim, or a forget made anywhere else must show here without leaving
    // and reopening the section.
    val version by ContactStore.changes.collectAsState()
    // The worn compartment's contacts, the chat list's own rule (§1.1a):
    // one hat, one list, and the single-persona era sees no change. The
    // caption below is why the list can be shorter than the phone's whole
    // address book without reading as data loss.
    val personas = remember { PersonaStore(context) }
    val scoped = remember(version) { personas.all().size > 1 }
    fun scopedAll(): List<Contact> = store.all().let { all ->
        if (personas.all().size > 1) {
            val worn = personas.worn()
            all.filter { personas.ownerHexOf(it) == worn }
        } else all
    }
    var contacts by remember(version) { mutableStateOf(scopedAll()) }
    var confirm by remember { mutableStateOf<Contact?>(null) }
    var profileOf by remember { mutableStateOf<Contact?>(null) }
    val wornName = remember(version) {
        personas.all().firstOrNull { it.hex == personas.worn() }
            ?.name?.ifBlank { null }
            ?: context.getString(R.string.personas_primary)
    }

    profileOf?.let { p ->
        // A full-screen overlay, not a nested screen: nesting put a second
        // top bar under the section's own and left the system back button
        // wired to neither — it exited to Home past both arrows. A dialog
        // carries exactly one header, covers the bar beneath instead of
        // stacking, and hands its dismissal to the back button for free.
        androidx.compose.ui.window.Dialog(
            onDismissRequest = { profileOf = null; contacts = scopedAll() },
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
                    onBack = { profileOf = null; contacts = scopedAll() },
                    onOpenChat = { profileOf = null; onOpenChat(it) },
                )
            }
        }
    }

    if (contacts.isEmpty()) {
        Box(Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Text(
                    stringResource(R.string.drawer_no_contacts),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                if (scoped) {
                    Spacer(Modifier.height(8.dp))
                    Text(
                        stringResource(R.string.personas_contacts_scope, wornName),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        return
    }

    // Which row is showing its menu. One at a time, by persona, so the menu
    // survives the list re-sorting underneath it.
    var menuFor by remember { mutableStateOf<String?>(null) }

    LazyColumn(Modifier.fillMaxSize()) {
        if (scoped) {
            item(key = "scope-note") {
                Text(
                    stringResource(R.string.personas_contacts_scope, wornName),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                )
            }
        }
        items(contacts, key = { it.personaHex }) { c ->
            Box {
                ListItem(
                    // The same avatar the chat list and the bar tab draw —
                    // this was the only list of people in the app without
                    // one, which is why it read as a table of hex rather
                    // than a list of somebodies.
                    leadingContent = { Avatar(c.displayName(), c.avatar) },
                    headlineContent = { Text(c.displayName()) },
                    supportingContent = { Text(personaGroups(c.personaHex)) },
                    // Transparent, like the chat list and the activity list.
                    // A ListItem defaults to `surface`, which is not the page
                    // it sits on, so the rows drew a band of a slightly
                    // different grey that stopped dead under the last one —
                    // a block of list floating on the screen rather than a
                    // list on the page.
                    colors = ListItemDefaults.colors(
                        containerColor = androidx.compose.ui.graphics.Color.Transparent,
                    ),
                    modifier = Modifier.combinedClickable(
                        // Long press rather than a row of icons: the destructive
                        // one was sitting a mistap away from a contact's name,
                        // permanently, in the error colour.
                        onClick = { profileOf = c },
                        onLongClick = { menuFor = c.personaHex },
                    ),
                )
                DropdownMenu(
                    expanded = menuFor == c.personaHex,
                    onDismissRequest = { menuFor = null },
                ) {
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.drawer_open_chat)) },
                        leadingIcon = { Icon(Icons.Filled.ChatBubbleOutline, null) },
                        onClick = { menuFor = null; onOpenChat(c) },
                    )
                    DropdownMenuItem(
                        text = {
                            Text(
                                stringResource(R.string.drawer_delete_contact),
                                color = MaterialTheme.colorScheme.error,
                            )
                        },
                        leadingIcon = {
                            Icon(
                                Icons.Filled.DeleteOutline, null,
                                tint = MaterialTheme.colorScheme.error,
                            )
                        },
                        onClick = { menuFor = null; confirm = c },
                    )
                }
            }
            HorizontalDivider()
        }
    }

    confirm?.let { c ->
        AlertDialog(
            onDismissRequest = { confirm = null },
            title = { Text(stringResource(R.string.drawer_forget_title, isolate(c.displayName()))) },
            text = { Text(stringResource(R.string.drawer_forget_body)) },
            confirmButton = {
                TextButton(onClick = {
                    store.forget(c.personaHex)
                    // Still the worn hat's list — this was the whole book,
                    // every other persona's contacts included, and only
                    // the store's bump re-scoping it kept that off screen.
                    contacts = scopedAll()
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


/**
 * A persona as something two people can read to each other.
 *
 * The same characters, in fours. Comparing `15e9b2cfce73c7b6fc4e0d3e` against
 * another phone means holding a place in an unbroken run of hex and losing it;
 * `15e9 b2cf ce73 c7b6 fc4e 0d3e` is six short words, which is how every other
 * fingerprint anybody has ever had to check aloud is written. Nothing is
 * hidden that was not hidden before — the full value is on the profile.
 */
private fun personaGroups(hex: String): String =
    hex.take(24).chunked(4).joinToString(" ") + "…"

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

    // Arming a kiosk requires a PIN to already exist.
    //
    // The staff door is the only way out of kiosk mode, and the gate behind it
    // offers to *set* a PIN when the phone has none — which is right
    // everywhere else, because the alternative is locking an owner out of
    // their own wallet. On a counter nobody is standing at, it means the next
    // customer chooses the PIN and walks straight through into the wallet and
    // the chats. So the question is asked here instead, at the one moment the
    // owner is provably the person holding the phone.
    var pinFor by remember { mutableStateOf<org.ducatproject.ducat.Mode?>(null) }

    fun choose(m: org.ducatproject.ducat.Mode) {
        val armed = current == org.ducatproject.ducat.Mode.Kiosk
        if (m == org.ducatproject.ducat.Mode.Kiosk &&
            !org.ducatproject.ducat.Pin.isSet(context)
        ) {
            pinFor = m
        } else if (armed && m != org.ducatproject.ducat.Mode.Kiosk) {
            // Leaving is the staff door's question, whichever way this
            // list was reached. The shell no longer lets a kiosk open the
            // drawer at all; this is the same rule kept at the picker.
            pinFor = m
        } else {
            pick(m)
        }
    }

    PinGate(
        open = pinFor != null,
        onDismiss = { pinFor = null },
        onPassed = { pinFor?.let { pick(it) }; pinFor = null },
    )

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
            org.ducatproject.ducat.Mode.Press,
            stringResource(R.string.mode_press),
            stringResource(R.string.mode_press_desc),
        ),
        Triple(
            org.ducatproject.ducat.Mode.Marketplace,
            stringResource(R.string.mode_marketplace),
            stringResource(R.string.mode_marketplace_desc),
        ),
        Triple(
            org.ducatproject.ducat.Mode.Renting,
            stringResource(R.string.mode_renting),
            stringResource(R.string.mode_renting_desc),
        ),
        Triple(
            org.ducatproject.ducat.Mode.HireHelp,
            stringResource(R.string.mode_hire_help),
            stringResource(R.string.mode_hire_help_desc),
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

        options.forEach { (mode, title, detail) ->
            val active = current == mode
            Card(
                Modifier.fillMaxWidth().padding(bottom = 10.dp),
                colors = CardDefaults.cardColors(
                    containerColor = if (active) {
                        MaterialTheme.colorScheme.primaryContainer
                    } else {
                        MaterialTheme.colorScheme.surfaceContainerHigh
                    },
                ),
            ) {
                Row(
                    Modifier.fillMaxWidth()
                        .clickable { choose(mode) }
                        .padding(horizontal = 16.dp, vertical = 14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(
                        Modifier.size(42.dp)
                            .clip(CircleShape)
                            .background(
                                if (active) {
                                    MaterialTheme.colorScheme.primary
                                } else {
                                    MaterialTheme.colorScheme.surfaceVariant
                                },
                            ),
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            modeIcon(mode),
                            contentDescription = null,
                            tint = if (active) {
                                MaterialTheme.colorScheme.onPrimary
                            } else {
                                MaterialTheme.colorScheme.onSurfaceVariant
                            },
                        )
                    }
                    Spacer(Modifier.width(14.dp))
                    Column(Modifier.weight(1f)) {
                        Text(title, style = MaterialTheme.typography.titleMedium)
                        Text(
                            detail,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        // Which hat the shift starts in (§15.11 meets the
                        // roster): a bar answering as the bar however the
                        // phone arrived. Only once a second persona
                        // exists — one persona has nothing to choose.
                        ModePersonaBinding(mode)
                    }
                    if (active) {
                        Spacer(Modifier.width(8.dp))
                        Icon(
                            Icons.Filled.CheckCircle,
                            contentDescription = null,
                            tint = MaterialTheme.colorScheme.primary,
                        )
                    }
                }
            }
        }
    }
}

private fun modeIcon(mode: org.ducatproject.ducat.Mode) = when (mode) {
    org.ducatproject.ducat.Mode.None -> Icons.Filled.Person
    org.ducatproject.ducat.Mode.Pos -> Icons.Filled.PointOfSale
    org.ducatproject.ducat.Mode.BarTab -> Icons.Filled.LocalBar
    org.ducatproject.ducat.Mode.Taxi -> Icons.Filled.LocalTaxi
    org.ducatproject.ducat.Mode.Donate -> Icons.Filled.VolunteerActivism
    org.ducatproject.ducat.Mode.Renting -> Icons.Filled.House
    org.ducatproject.ducat.Mode.Kiosk -> Icons.Filled.Storefront
    org.ducatproject.ducat.Mode.Marketplace -> Icons.Filled.Sell
    org.ducatproject.ducat.Mode.HireHelp -> Icons.Filled.Handyman
    org.ducatproject.ducat.Mode.Press -> Icons.AutoMirrored.Filled.MenuBook
}

/** Opens the sealed-room viewer for a fetched site. Injected: WebView is
 *  the phone's business (see MainActivity); the desk compiles a no-op. */
var siteOpen: (android.content.Context, String) -> Unit = { _, _ -> }

/** A ducat:site/ address arriving from a deep link or paste, waiting for
 *  the section to add it. */
val pendingSiteAdd = kotlinx.coroutines.flow.MutableStateFlow<String?>(null)

/** The [ThreadSends] key the add runs under — one at a time, and a word a
 *  record key can never be. */
private const val SITE_ADD_JOB = "add"

/**
 * §16.22 on the phone: saved sites, each a stable address whose bundle
 * travels whole and renders in a sealed room. Adding is pasting an
 * address; opening fetches if the head moved and hands the bundle to the
 * viewer; keeping one alive is the mirroring gift, given knowingly.
 */
@Composable
fun SitesSection() {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val sites = remember(version) { org.ducatproject.ducat.Sites.all(context) }
    var adding by remember { mutableStateOf(false) }
    var addText by remember { mutableStateOf("") }
    // The add and each site's fetch run under process-level jobs, keyed
    // "site:<record key>" (the add under a word no key can be), and this
    // section reads which one is out rather than owning it. They ran in
    // this section's scope: a rotation or a call while a bundle of
    // hundreds of files came down took the scope with it, the fetch
    // finished into the store, and the hand-off to the viewer — a
    // `withContext(Main)` — was the one line cancellation could skip. The
    // site was fetched and never opened, over a button that offered to
    // fetch it again. Whoever is up when it lands opens it.
    fun siteJobs(): List<String> = listOf(SITE_ADD_JOB) + sites.map { it.recordKey }
    var busy by remember {
        mutableStateOf(siteJobs().firstOrNull { ThreadSends.inFlight("site:$it") })
    }
    var word by remember { mutableStateOf<String?>(null) }
    val tick by ThreadSends.ticks.collectAsState()
    LaunchedEffect(tick, sites) {
        val keys = siteJobs()
        busy = keys.firstOrNull { ThreadSends.inFlight("site:$it") }
        for (k in keys) for (o in ThreadSends.take("site:$k")) when (o) {
            is ThreadSends.Outcome.Landed -> {
                word = null
                o.result?.let { siteOpen(context, it) }
            }
            is ThreadSends.Outcome.Failed -> {
                word = o.error.saidWhy() ?: o.error.javaClass.simpleName
            }
        }
    }

    fun addByUri(uri: String) {
        val rec = org.ducatproject.ducat.Sites.parseUri(uri.trim())
        if (rec == null) {
            word = context.getString(R.string.sites_bad_uri)
            return
        }
        busy = SITE_ADD_JOB
        ThreadSends.launch(ContactStore(context), "site:$SITE_ADD_JOB", null) {
            org.ducatproject.ducat.Sites.add(context, rec)
            null
        }
    }

    // A link is not a paste. Unlike an address typed in here, a link can be
    // sent by anyone, and a site is a page that gets opened — so the address
    // is shown and the person asked, the way a tapped card is (MainActivity).
    var linkAsk by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(Unit) {
        pendingSiteAdd.collect { uri ->
            if (uri != null) {
                pendingSiteAdd.value = null
                linkAsk = uri
            }
        }
    }
    linkAsk?.let { uri ->
        AlertDialog(
            onDismissRequest = { linkAsk = null },
            title = { Text(stringResource(R.string.sites_add_link_title)) },
            text = {
                Column {
                    Text(stringResource(R.string.sites_add_link_body))
                    Spacer(Modifier.height(8.dp))
                    Text(
                        uri,
                        fontFamily = FontFamily.Monospace,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            },
            confirmButton = {
                TextButton(onClick = { linkAsk = null; addByUri(uri) }) {
                    Text(stringResource(R.string.sites_add_confirm))
                }
            },
            dismissButton = {
                TextButton(onClick = { linkAsk = null }) {
                    Text(stringResource(R.string.common_cancel))
                }
            },
        )
    }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp)) {
        Text(
            stringResource(R.string.sites_intro),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(12.dp))

        if (sites.isEmpty()) {
            Column(
                Modifier.fillMaxWidth().padding(top = 48.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Box(
                    Modifier.size(72.dp).clip(CircleShape)
                        .background(MaterialTheme.colorScheme.surfaceVariant),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        Icons.Filled.Public, null, Modifier.size(36.dp),
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Spacer(Modifier.height(16.dp))
                Text(
                    stringResource(R.string.sites_empty_title),
                    style = MaterialTheme.typography.titleLarge,
                )
                Spacer(Modifier.height(8.dp))
                Text(
                    stringResource(R.string.sites_empty_body),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
                Spacer(Modifier.height(20.dp))
                Button(onClick = { adding = true }) {
                    Text(stringResource(R.string.sites_add))
                }
            }
        } else {
            sites.forEach { site ->
                Card(
                    Modifier.fillMaxWidth().padding(bottom = 10.dp),
                    colors = CardDefaults.cardColors(
                        containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                    ),
                ) {
                    Column(Modifier.padding(14.dp)) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Column(Modifier.weight(1f)) {
                                Text(site.title, style = MaterialTheme.typography.titleMedium)
                                Text(
                                    if (busy == site.recordKey) {
                                        stringResource(R.string.sites_fetching)
                                    } else if (site.fetchedDigestHex == null) {
                                        stringResource(R.string.sites_not_fetched)
                                    } else if (site.fetchedDigestHex != site.digestHex) {
                                        stringResource(R.string.sites_update_waiting)
                                    } else {
                                        stringResource(R.string.sites_offline_ready)
                                    },
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                            Button(
                                enabled = busy == null,
                                onClick = {
                                    busy = site.recordKey
                                    ThreadSends.launch(
                                        ContactStore(context), "site:${site.recordKey}", null,
                                    ) {
                                        // Head first: an updated site is
                                        // noticed at the door, fetched
                                        // behind it, rendered fresh.
                                        val latest = runCatching {
                                            org.ducatproject.ducat.Sites.add(
                                                context, site.recordKey,
                                            )
                                        }.getOrDefault(site)
                                        org.ducatproject.ducat.Sites.fetchBundle(
                                            context, latest,
                                        )
                                        // The landing opens it — on the
                                        // main thread, from whichever
                                        // section is up to hear it.
                                        site.recordKey
                                    }
                                },
                            ) { Text(stringResource(R.string.sites_open)) }
                        }
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            // Off the main thread, both: the store is an
                            // encrypted table read back whole, and remove
                            // deletes a bundle that can be hundreds of files.
                            Checkbox(
                                checked = site.keepAlive,
                                onCheckedChange = { keep ->
                                    ThreadSends.launch(
                                        ContactStore(context), "site:${site.recordKey}", null,
                                    ) {
                                        org.ducatproject.ducat.Sites.setKeepAlive(
                                            context, site.recordKey, keep,
                                        )
                                        null
                                    }
                                },
                            )
                            Text(
                                stringResource(R.string.sites_keep_alive),
                                style = MaterialTheme.typography.bodySmall,
                                modifier = Modifier.weight(1f),
                            )
                            TextButton(onClick = {
                                ThreadSends.launch(
                                    ContactStore(context), "site:${site.recordKey}", null,
                                ) {
                                    org.ducatproject.ducat.Sites.remove(context, site.recordKey)
                                    null
                                }
                            }) { Text(stringResource(R.string.sites_remove)) }
                        }
                    }
                }
            }
            TextButton(onClick = { adding = true }) {
                Text(stringResource(R.string.sites_add))
            }
        }

        word?.let {
            Spacer(Modifier.height(8.dp))
            Text(
                it,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
            )
        }
    }

    if (adding) {
        AlertDialog(
            onDismissRequest = { adding = false },
            title = { Text(stringResource(R.string.sites_add)) },
            text = {
                OutlinedTextField(
                    addText, { addText = it },
                    label = { Text(stringResource(R.string.sites_uri_label)) },
                    singleLine = true,
                )
            },
            confirmButton = {
                TextButton(
                    enabled = addText.isNotBlank(),
                    onClick = {
                        adding = false
                        addByUri(addText)
                        addText = ""
                    },
                ) { Text(stringResource(R.string.sites_add_confirm)) }
            },
            dismissButton = {
                TextButton(onClick = { adding = false }) {
                    Text(stringResource(R.string.common_cancel))
                }
            },
        )
    }
}

/**
 * Everything this phone offers, one desk: the listings across every kind
 * (a room, a car, a kayak, a bicycle for sale, an afternoon's help) and
 * the publications. Management without a mode switch — a shift is for
 * working a counter, not for editing a price.
 */
@Composable
fun SellingSection() {
    var tab by rememberSaveable { mutableStateOf(0) }
    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            FilterChip(
                selected = tab == 0,
                onClick = { tab = 0 },
                label = { Text(stringResource(R.string.shells_tab_listings)) },
            )
            FilterChip(
                selected = tab == 1,
                onClick = { tab = 1 },
                label = { Text(stringResource(R.string.section_publishing)) },
            )
        }
        if (tab == 0) {
            RentingScreen(kinds = org.ducatproject.ducat.Listings.KINDS)
        } else {
            PublishingSection()
        }
    }
}

@Composable
private fun ModePersonaBinding(mode: org.ducatproject.ducat.Mode) {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val personas = remember { PersonaStore(context) }
    val roster = remember(version) { personas.all() }
    if (roster.size < 2) return
    val modes = remember { org.ducatproject.ducat.ModeStore(context) }
    val bound = remember(version) { modes.boundPersona(mode) }
    var open by remember { mutableStateOf(false) }

    val nameOf: (String) -> String = { hex ->
        roster.firstOrNull { it.hex == hex }?.name?.ifBlank { null }
            ?: context.getString(R.string.personas_primary)
    }
    Box {
        // A control, dressed as one: the bare text link sat exactly where
        // a thumb taps the card to choose the mode, and swallowed the tap.
        // The arrow says "this opens something" and the row hugs its text.
        Row(
            Modifier.clickable { open = true }.padding(top = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                if (bound != null) {
                    stringResource(R.string.personas_mode_answers, nameOf(bound))
                } else {
                    stringResource(R.string.personas_mode_answers_worn)
                },
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.primary,
            )
            Icon(
                Icons.Filled.ArrowDropDown,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.primary,
                modifier = Modifier.size(16.dp),
            )
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(
                text = { Text(stringResource(R.string.personas_mode_answers_worn)) },
                onClick = { modes.bindPersona(mode, null); open = false },
            )
            roster.forEach { p ->
                DropdownMenuItem(
                    text = { Text(nameOf(p.hex)) },
                    onClick = { modes.bindPersona(mode, p.hex); open = false },
                )
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

/**
 * The counter's sales tax (see [org.ducatproject.ducat.Tax]).
 *
 * A percentage, once, instead of an amount per sale: the till, the bar tab
 * and the kiosk compute the figure from the subtotal in front of the
 * customer, it rides the bill and the receipt as the `tax` line core already
 * checks the arithmetic of, and the CSV export carries it per transaction —
 * which is the half a business actually files.
 */
@Composable
internal fun TaxSetting() {
    val context = LocalContext.current
    var on by remember { mutableStateOf(org.ducatproject.ducat.Tax.enabled(context)) }
    var pct by remember {
        mutableStateOf(
            org.ducatproject.ducat.Tax.basisPoints(context)
                .takeIf { it > 0 }
                ?.let { org.ducatproject.ducat.Tax.percentText(it) } ?: "",
        )
    }
    fun push() {
        org.ducatproject.ducat.Tax.set(
            context, on, org.ducatproject.ducat.Tax.parsePercent(pct) ?: 0,
        )
    }
    Column {
        Text(
            stringResource(R.string.settings_tax_title),
            style = MaterialTheme.typography.titleMedium,
        )
        Spacer(Modifier.height(4.dp))
        Text(
            stringResource(R.string.settings_tax_note),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(8.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Switch(checked = on, onCheckedChange = { on = it; push() })
            Spacer(Modifier.width(12.dp))
            if (on) {
                OutlinedTextField(
                    value = pct,
                    onValueChange = {
                        pct = it.filter { c -> org.ducatproject.ducat.Amounts.isNumberChar(c) }
                        push()
                    },
                    label = { Text(stringResource(R.string.settings_tax_percent)) },
                    singleLine = true,
                    keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                        keyboardType = androidx.compose.ui.text.input.KeyboardType.Decimal,
                    ),
                    modifier = Modifier.width(140.dp),
                )
            }
        }
    }
}

/**
 * Every standing schedule, in one place. Creation lives on the request
 * form (where the first bill is typed); this is where they are found
 * again and stopped — because a cadence someone set in March must not
 * require remembering in September which thread it was set from.
 * Renders nothing while nothing recurs.
 */
@Composable
private fun RecurringSetting() {
    val context = LocalContext.current
    val version by org.ducatproject.ducat.ContactStore.changes.collectAsState()
    val bills = remember(version) { org.ducatproject.ducat.Recurring.all(context) }
    if (bills.isEmpty()) return
    val contacts = remember(version) { org.ducatproject.ducat.ContactStore(context).all() }
    Column {
        Text(
            stringResource(R.string.settings_recur_title),
            style = MaterialTheme.typography.titleMedium,
        )
        Spacer(Modifier.height(4.dp))
        bills.forEach { b ->
            val name = contacts.firstOrNull { it.personaHex == b.personaHex }
                ?.displayName() ?: "${b.personaHex.take(8)}…"
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(
                        isolate(name) + " — " +
                            org.ducatproject.ducat.Amounts.show(context, b.amountPxmr).primary,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    Text(
                        stringResource(
                            if (b.monthly) R.string.pay_repeat_monthly
                            else R.string.pay_repeat_weekly,
                        ) + " · " + stringResource(
                            R.string.settings_recur_next,
                            java.text.DateFormat.getDateInstance(java.text.DateFormat.MEDIUM)
                                .format(java.util.Date(b.nextAt)),
                        ),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                TextButton(onClick = { org.ducatproject.ducat.Recurring.stop(context, b.id) }) {
                    Text(stringResource(R.string.settings_recur_stop))
                }
            }
        }
        Spacer(Modifier.height(24.dp))
    }
}
