package org.ducatproject.ducat

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.platform.LocalContext
import org.ducatproject.ducat.ui.BackupSettings
import org.ducatproject.ducat.ui.BalanceCard
import org.ducatproject.ducat.ui.NetworkPanel
import org.ducatproject.ducat.ui.Onboarding
import org.ducatproject.ducat.ui.OnboardingFlow
import org.ducatproject.ducat.ui.Step
import org.ducatproject.ducat.ui.DucatTheme
import org.ducatproject.ducat.ui.ThemeMode
import org.ducatproject.ducat.ui.ThemePreference
import org.ducatproject.ducat.ui.ducat
import org.ducatproject.ducat.ui.BridgeSelfTest
import org.ducatproject.ducat.ui.ChatScreen
import org.ducatproject.ducat.ui.ContactsScreen
import uniffi.ducat_mobile.approxPaymentsSupported
import uniffi.ducat_mobile.protocolVersion

/**
 * One activity, one app.
 *
 * The navigation follows PayPal's — a hamburger for the long tail, a bottom bar
 * with an elevated centre action — because that shape is familiar to hundreds of
 * millions of people and there is no advantage in being novel about navigation.
 *
 * The centre action is **Send / Request**, and that is not a UI invention: it is
 * §15.2's `presenter_role`. Request means *I present and you tap me* (the POS
 * direction); Send means *I read your tap*. Both run end to end, and they are
 * **not symmetric** — the presenter supplies reachability, so the reader drives
 * every round trip. A screen written against one and reused for the other hangs.
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val prefs = ThemePreference(this)
        setContent {
            // Follows the system unless the user has said otherwise (Menu).
            var mode by remember { mutableStateOf(prefs.mode) }
            var onboarded by remember { mutableStateOf(prefs.onboarded) }
            var setup by remember { mutableStateOf(Onboarding()) }

            DucatTheme(mode) {
                if (!onboarded) {
                    // Nothing behind this until it is done. §4.3's backup step is
                    // worthless if the user can reach a funded wallet without it.
                    OnboardingFlow(setup) { next ->
                        setup = next
                        if (next.step == Step.Done && next.backupConfirmed) {
                            prefs.onboarded = true
                            onboarded = true
                        }
                    }
                } else {
                    DucatApp(
                        themeMode = mode,
                        onThemeChange = { mode = it; prefs.mode = it },
                    )
                }
            }
        }
    }
}

/** A screen that takes over from the tabbed shell. */
private sealed interface Overlay {
    data object None : Overlay
    data object Contacts : Overlay
    data class Chat(val contact: Contact) : Overlay
}

enum class Tab(val label: String) { Home("Home"), Accounts("Accounts"), Activity("Activity"), Menu("Menu") }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DucatApp(themeMode: ThemeMode, onThemeChange: (ThemeMode) -> Unit) {
    var tab by remember { mutableStateOf(Tab.Home) }
    // Contacts and a conversation are full-screen rather than a fifth tab. The
    // nav bar is the money surface; people are a place you go *into*, and
    // burying chat behind a bar slot would make the bar the wrong shape for the
    // one verb (§15.2's present/read) it exists to keep prominent.
    var overlay by remember { mutableStateOf<Overlay>(Overlay.None) }
    val context = LocalContext.current
    val persona = remember { PersonaStore(context).secret() }

    when (val o = overlay) {
        is Overlay.Chat -> {
            ChatScreen(o.contact) { overlay = Overlay.Contacts }
            return
        }
        Overlay.Contacts -> {
            Scaffold(
                topBar = {
                    TopAppBar(
                        title = { Text("People") },
                        navigationIcon = {
                            IconButton(onClick = { overlay = Overlay.None }) {
                                Icon(Icons.Filled.ArrowBack, contentDescription = "Back")
                            }
                        },
                    )
                },
            ) { padding ->
                Box(Modifier.padding(padding)) {
                    ContactsScreen(persona) { overlay = Overlay.Chat(it) }
                }
            }
            return
        }
        Overlay.None -> {}
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(if (tab == Tab.Home) "" else tab.label) },
                navigationIcon = {
                    IconButton(onClick = { tab = Tab.Menu }) {
                        Icon(Icons.Filled.Menu, contentDescription = "Menu")
                    }
                },
                actions = {
                    IconButton(onClick = { overlay = Overlay.Contacts }) {
                        Icon(Icons.Filled.People, contentDescription = "People")
                    }
                },
            )
        },
        bottomBar = {
            // PayPal seats its centre action *inside* the bar. Floating it above
            // was a real bug on a real screen: the button covered the card
            // beneath it, and on a payment screen the thing being covered is a
            // number someone is about to act on.
            NavigationBar {
                NavItem(Tab.Home, Icons.Filled.Home, tab) { tab = it }
                // The coin, which we already draw for the launcher. Nothing in
                // the core icon set means "your money" without borrowing a
                // shopping metaphor this app is not about.
                NavigationBarItem(
                    selected = tab == Tab.Accounts,
                    onClick = { tab = Tab.Accounts },
                    icon = {
                        Icon(
                            painterResource(R.drawable.ic_ducat_coin),
                            contentDescription = Tab.Accounts.label,
                            modifier = Modifier.size(24.dp),
                        )
                    },
                    label = { Text(Tab.Accounts.label) },
                )

                // The one verb that dominates, raised without leaving the bar.
                // It is `presenter_role` (§15.2), not a mode we invented:
                // Request means I present and you tap me, Send means I read
                // your tap.
                // NOT fillMaxHeight: NavigationBar does not constrain its
                // children's height, so filling it expanded the bar to the whole
                // screen and left the content with none. Shipped once, visible
                // immediately on a device and invisible from here.
                Box(
                    Modifier.weight(1f),
                    contentAlignment = Alignment.Center,
                ) {
                    FloatingActionButton(
                        onClick = { /* tap flow */ },
                        shape = CircleShape,
                        modifier = Modifier.size(52.dp),
                    ) {
                        Icon(Icons.Filled.SwapVert, contentDescription = "Send or request")
                    }
                }

                NavItem(Tab.Activity, Icons.Filled.Receipt, tab) { tab = it }
                NavItem(Tab.Menu, Icons.Filled.Settings, tab) { tab = it }
            }
        },
    ) { padding ->
        Column(
            Modifier
                .padding(padding)
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
        ) {
            when (tab) {
                Tab.Home -> HomeScreen()
                Tab.Accounts -> Placeholder(
                    "Accounts",
                    "Float, reserve and bond — §17.2 forbids showing them as one number."
                )
                Tab.Activity -> Placeholder(
                    "Activity",
                    "Mostly nameless by design: a name exists only where §16.3's " +
                        "post-receipt coda established a contact."
                )
                Tab.Menu -> MenuScreen(themeMode, onThemeChange)
            }
        }
    }
}

@Composable
private fun HomeScreen() {
    // The wallet figures are still placeholders — they arrive from a Monero
    // wallet, which is the next piece. **The capacity is not a placeholder.**
    // It comes from `core::float` across the bridge, so the one number §17.2
    // forbids overstating is computed by the same code the conformance vectors
    // and the harness run, rather than by a second implementation in Kotlin.
    val float = Float(
        spendablePxmr = 40_000_000_000,
        lockedPxmr = 12_000_000_000,
        blocksToUnlock = 7,
        unlockedOutputs = 4,
    )
    val approx = remember(float.unlockedOutputs) {
        approxPaymentsSupported(float.unlockedOutputs.toUInt()).toInt()
    }
    BalanceCard(
        spendable = Money(4000),
        capacity = Capacity(approxPayments = approx),
        float = float,
        locked = Money(1200),
        onTopUp = {},
    )
    // On the home screen for this build only. It answers the one question this
    // APK exists to answer, and it should be deleted the moment it has.
    BridgeSelfTest()
}

/** One bar slot, so the centre action can sit among them rather than over them. */
@Composable
private fun RowScope.NavItem(
    target: Tab,
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    current: Tab,
    onSelect: (Tab) -> Unit,
) {
    NavigationBarItem(
        selected = current == target,
        onClick = { onSelect(target) },
        icon = { Icon(icon, contentDescription = target.label) },
        label = { Text(target.label) },
    )
}

@Composable
private fun MenuScreen(mode: ThemeMode, onChange: (ThemeMode) -> Unit) {
    val context = LocalContext.current
    Column(Modifier.fillMaxWidth().padding(20.dp)) {
        // The transport, first: nothing else here matters if a route cannot be
        // built, and this is the screen a person troubleshoots from.
        NetworkPanel(storageDir = context.filesDir.absolutePath + "/veilid")
        BackupSettings(spendKeyHex = null, restoreHeight = 0uL, personaSecret = null)
        Spacer(Modifier.height(16.dp))
        Text("Appearance", style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(8.dp))
        ThemeMode.entries.forEach { m ->
            Row(
                Modifier.fillMaxWidth().padding(vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(selected = mode == m, onClick = { onChange(m) })
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
        Spacer(Modifier.height(24.dp))
        Text(
            "Personas, backup, custody, verification thresholds, markets, relays, records.",
            style = MaterialTheme.typography.bodyMedium,
        )
        Spacer(Modifier.height(8.dp))
        // Proof the native core is loaded and answering.
        Text("speaking ${protocolVersion()}", style = MaterialTheme.typography.bodySmall)
    }
}

@Composable
private fun Placeholder(title: String, note: String) {
    Column(
        Modifier.fillMaxWidth().padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(title, style = MaterialTheme.typography.headlineSmall)
        Spacer(Modifier.height(8.dp))
        Text(note, style = MaterialTheme.typography.bodyMedium)
    }
}
