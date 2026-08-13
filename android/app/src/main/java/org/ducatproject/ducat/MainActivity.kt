package org.ducatproject.ducat

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.clickable
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
import org.ducatproject.ducat.ui.QrHub
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
                            // The profile choices are settings, not keys, and
                            // are kept where the rest of the app reads them.
                            next.displayName?.let {
                                NameStore(this@MainActivity).put(it)
                            }
                            ContactStore(this@MainActivity)
                                .setPublishAddress(next.publishPayto)
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
    var payAddress by remember { mutableStateOf<String?>(null) }
    var qrOpen by remember { mutableStateOf(false) }
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
                            colors = TopAppBarDefaults.topAppBarColors(
                                containerColor = MaterialTheme.colorScheme.background,
                            ),
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
        if (payOpen) PaySheet(prefillAddress = payAddress) { payOpen = false; payAddress = null }

        // Venmo puts this in the corner of every screen, and the reason it
        // works is that "show me yours / here is mine" is one gesture between
        // two people standing together — not two features in two menus.
        if (qrOpen) {
            QrHub(
                onOpenChat = { qrOpen = false; overlay = Overlay.Chat(it) },
                onScanAddress = { qrOpen = false; payAddress = it; payOpen = true },
                onClose = { qrOpen = false },
            )
        }

        Scaffold(
            topBar = {
                TopAppBar(
                    colors = TopAppBarDefaults.topAppBarColors(
                        containerColor = MaterialTheme.colorScheme.background,
                    ),
                    title = {
                        if (tab != Tab.Home) {
                            Text(tab.label, style = MaterialTheme.typography.titleLarge)
                        }
                    },
                    navigationIcon = {
                        IconButton(onClick = { scope.launch { drawer.open() } }) {
                            Icon(Icons.Filled.Menu, contentDescription = "Menu")
                        }
                    },
                    actions = {
                        IconButton(onClick = { qrOpen = true }) {
                            Icon(Icons.Filled.QrCode2, contentDescription = "Codes")
                        }
                        // Your own face, where Venmo puts it, and it goes where
                        // a face should: the profile. Re-read on store changes
                        // so a newly set picture appears without a restart.
                        val pv by ContactStore.changes.collectAsState()
                        val me = remember(pv) { MyProfile(context) }
                        Box(
                            Modifier.padding(end = 8.dp)
                                .clickable { overlay = Overlay.Drawer(Section.Profile) },
                        ) {
                            org.ducatproject.ducat.ui.Avatar(
                                me.name() ?: "?", me.avatar(), size = 32,
                            )
                        }
                    },
                )
            },
            bottomBar = {
                // Venmo's bar: the app's own mascot on the centre circle, a
                // size up from the tabs, overhanging the bar's top edge. The
                // overhang cannot live *inside* NavigationBar — it clips its
                // children, which cut the circle flat — so the bar sits in a
                // wrapper with a transparent strip for the circle to rise into.
                // The strip is part of the bottomBar, so the Scaffold keeps
                // content clear of it: an overhang, not the free-floating FAB
                // this once was, which covered the number someone was about to
                // act on.
                Box(Modifier.fillMaxWidth()) {
                    Column {
                        Spacer(Modifier.height(18.dp))
                        NavigationBar(
                            containerColor = MaterialTheme.colorScheme.background,
                            tonalElevation = 0.dp,
                        ) {
                            NavItem(Tab.Home, Icons.Filled.Home, tab) { tab = it }
                            // A wallet, since the mark owns the centre. Two
                            // copies of it in one bar and neither reads as the
                            // mark.
                            NavItem(Tab.Accounts, Icons.Filled.AccountBalanceWallet, tab) { tab = it }
                            // The centre slot: only the label lives in the bar;
                            // the circle floats in the wrapper above.
                            Column(
                                Modifier.weight(1f).fillMaxHeight(),
                                horizontalAlignment = Alignment.CenterHorizontally,
                                verticalArrangement = Arrangement.Bottom,
                            ) {
                                Text(
                                    "Send/Receive",
                                    style = MaterialTheme.typography.labelSmall,
                                    maxLines = 1,
                                    softWrap = false,
                                    modifier = Modifier.padding(bottom = 14.dp),
                                )
                            }
                            NavItem(Tab.Activity, Icons.Filled.Receipt, tab) { tab = it }
                            NavItem(Tab.Chat, Icons.Filled.ChatBubbleOutline, tab) { tab = it }
                        }
                    }
                    Surface(
                        onClick = { payOpen = true },
                        shape = CircleShape,
                        color = MaterialTheme.colorScheme.tertiary,
                        shadowElevation = 6.dp,
                        modifier = Modifier.size(62.dp).align(Alignment.TopCenter),
                    ) {
                        Box(contentAlignment = Alignment.Center) {
                            // An Image, not an Icon: Icon tints to one colour,
                            // and the cat is not the cat as a silhouette.
                            androidx.compose.foundation.Image(
                                painterResource(R.mipmap.ic_launcher),
                                contentDescription = "Send or receive",
                                modifier = Modifier.size(48.dp),
                            )
                        }
                    }
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
        sync = b,
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
