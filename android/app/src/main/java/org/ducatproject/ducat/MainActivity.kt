package org.ducatproject.ducat

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
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
import androidx.activity.compose.BackHandler
import kotlinx.coroutines.launch
import org.ducatproject.ducat.ui.AccountsScreen
import org.ducatproject.ducat.ui.ActivityScreen
import org.ducatproject.ducat.ui.PaySheet
import org.ducatproject.ducat.ui.ChatListScreen
import org.ducatproject.ducat.ui.ChatScreen
import org.ducatproject.ducat.ui.DrawerContent
import org.ducatproject.ducat.ui.Section
import org.ducatproject.ducat.ui.SectionScreen
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
        // Before super, so the splash is installed for this start rather than
        // the next one.
        installSplashScreen()
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
                            // Persist the wallet setup created. It used to live
                            // only in onboarding's Compose state, so the address
                            // a user was shown during setup vanished the moment
                            // setup finished — and BackupSettings was handed
                            // null for the very key it exists to back up.
                            next.wallet?.let { w ->
                                WalletStore(this@MainActivity).save(
                                    address = w.address,
                                    spendKeyHex = w.spendKeyHex,
                                    restoreHeight = w.restoreHeight,
                                    stagenet = true,
                                )
                            }
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
    data class Chat(val contact: Contact) : Overlay
    data class Drawer(val section: Section) : Overlay
}

enum class Tab(val label: String) {
    Home("Home"), Accounts("Accounts"), Activity("Activity"), Chat("Chat")
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DucatApp(themeMode: ThemeMode, onThemeChange: (ThemeMode) -> Unit) {
    var tab by remember { mutableStateOf(Tab.Home) }
    var overlay by remember { mutableStateOf<Overlay>(Overlay.None) }
    var payOpen by remember { mutableStateOf(false) }
    val drawer = rememberDrawerState(DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val persona = remember { PersonaStore(context).secret() }

    // Android's back gesture is a system behaviour, not a widget: without a
    // handler it goes straight to the activity and closes the app. Every screen
    // that is *entered* has to say how to leave it, or the way out of a
    // conversation is quitting.
    BackHandler(enabled = drawer.isOpen) { scope.launch { drawer.close() } }
    BackHandler(enabled = overlay !is Overlay.None && drawer.isClosed) {
        overlay = when (overlay) {
            // A chat opened from Contacts returns to Contacts, not to the tabs.
            is Overlay.Chat -> Overlay.None
            else -> Overlay.None
        }
    }
    BackHandler(enabled = overlay is Overlay.None && drawer.isClosed && tab != Tab.Home) {
        tab = Tab.Home
    }

    ModalNavigationDrawer(
        drawerState = drawer,
        drawerContent = {
            DrawerContent { section ->
                scope.launch { drawer.close() }
                overlay = Overlay.Drawer(section)
            }
        },
    ) {
        when (val o = overlay) {
            is Overlay.Chat -> {
                ChatScreen(o.contact) { overlay = Overlay.None }
                return@ModalNavigationDrawer
            }
            is Overlay.Drawer -> {
                Scaffold(
                    topBar = {
                        TopAppBar(
                            title = { Text(o.section.label) },
                            navigationIcon = {
                                IconButton(onClick = { overlay = Overlay.None }) {
                                    Icon(Icons.Filled.ArrowBack, contentDescription = "Back")
                                }
                            },
                        )
                    },
                ) { padding ->
                    Box(Modifier.padding(padding)) {
                        SectionScreen(o.section, themeMode, onThemeChange) {
                            overlay = Overlay.Chat(it)
                        }
                    }
                }
                return@ModalNavigationDrawer
            }
            Overlay.None -> {}
        }

        // The whole send/request flow: who first, then how much, with contacts
        // listed above an address field because a payment to a contact carries
        // a note and a thread and an address payment carries neither.
        if (payOpen) PaySheet { payOpen = false }

        Scaffold(
            topBar = {
                TopAppBar(
                    title = { Text(if (tab == Tab.Home) "" else tab.label) },
                    navigationIcon = {
                        IconButton(onClick = { scope.launch { drawer.open() } }) {
                            Icon(Icons.Filled.Menu, contentDescription = "Menu")
                        }
                    },
                )
            },
            bottomBar = {
                // PayPal seats its centre action *inside* the bar. Floating it
                // above was a real bug on a real screen: the button covered the
                // card beneath it, and on a payment screen the thing being
                // covered is a number someone is about to act on.
                NavigationBar {
                    NavItem(Tab.Home, Icons.Filled.Home, tab) { tab = it }
                    // The coin, which we already draw for the launcher. Nothing
                    // in the core icon set means "your money" without borrowing
                    // a shopping metaphor this app is not about.
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

                    // The one verb that dominates, raised without leaving the
                    // bar. It is `presenter_role` (§15.2), not a mode we
                    // invented: Request means I present and you tap me, Send
                    // means I read your tap.
                    // NOT fillMaxHeight: NavigationBar does not constrain its
                    // children's height, so filling it expanded the bar to the
                    // whole screen and left the content with none.
                    Box(Modifier.weight(1f), contentAlignment = Alignment.Center) {
                        FloatingActionButton(
                            onClick = { payOpen = true },
                            shape = CircleShape,
                            containerColor = MaterialTheme.colorScheme.tertiary,
                            contentColor = MaterialTheme.colorScheme.onTertiary,
                            modifier = Modifier.size(52.dp),
                        ) {
                            Icon(Icons.Filled.SwapVert, contentDescription = "Send or request")
                        }
                    }

                    NavItem(Tab.Activity, Icons.Filled.Receipt, tab) { tab = it }
                    NavItem(Tab.Chat, Icons.Filled.ChatBubbleOutline, tab) { tab = it }
                }
            },
        ) { padding ->
            Box(Modifier.padding(padding).fillMaxSize()) {
                when (tab) {
                    // Only the scrolling screens get a scroll wrapper. Chat owns
                    // a LazyColumn, and nesting one inside a vertical scroll
                    // gives it unbounded height — it renders every row at once
                    // and the list stops being lazy.
                    Tab.Home -> Column(Modifier.verticalScroll(rememberScrollState())) {
                        HomeScreen(onTopUp = { tab = Tab.Accounts })
                    }
                    Tab.Accounts -> AccountsScreen()
                    Tab.Activity -> ActivityScreen()
                    Tab.Chat -> ChatListScreen(persona) { overlay = Overlay.Chat(it) }
                }
            }
        }
    }
}

@Composable
private fun HomeScreen(onTopUp: () -> Unit) {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val b = remember(version) { Wallet.balances(context) }

    // The capacity comes from `core::float` across the bridge, so the one number
    // §17.2 forbids overstating is computed by the same code the conformance
    // vectors and the harness run, rather than a second implementation in Kotlin.
    val float = Float(
        spendablePxmr = b.spendablePxmr,
        lockedPxmr = b.lockedPxmr,
        blocksToUnlock = b.blocksToUnlock.toInt(),
        unlockedOutputs = b.spendableOutputs,
    )
    val approx = remember(b.spendableOutputs) {
        approxPaymentsSupported(b.spendableOutputs.toUInt()).toInt()
    }
    BalanceCard(
        spendablePxmr = b.spendablePxmr,
        capacity = Capacity(approxPayments = approx),
        float = float,
        locked = Money(b.lockedPxmr / 1_000_000L, symbol = "", exponent = 6),
        onTopUp = onTopUp,
    )
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
