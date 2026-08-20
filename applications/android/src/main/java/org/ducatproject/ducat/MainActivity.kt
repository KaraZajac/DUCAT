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
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.rememberSaveable
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
// A FragmentActivity rather than a ComponentActivity, which it extends:
// BiometricPrompt hosts itself in a fragment, and the system unlock prompt is
// worth the base class. Nothing else changes — setContent and the rest are
// ComponentActivity's and still apply.
class MainActivity : androidx.fragment.app.FragmentActivity() {
    companion object {
        /**
         * The thread a notification asked for. A flow rather than intent
         * plumbing through compose: the activity may already be alive
         * (singleTop), so onNewIntent has to reach a screen that mounted
         * long ago, and this is the only channel both paths share.
         */
        val openChat = kotlinx.coroutines.flow.MutableStateFlow<String?>(null)

        /** A tapped ducat: link, waiting for the shell to show who it is. */
        val claimLink = kotlinx.coroutines.flow.MutableStateFlow<String?>(null)
    }

    private fun readIntent(i: android.content.Intent?) {
        i?.getStringExtra("open_chat")?.let { openChat.value = it }
        // §18.7 token mode: the manifest registers ducat: links, and this URI
        // used to stop right here, read by nobody — a tapped card opened the
        // app to Home, silently. It now reaches the same claim the scanner
        // runs, behind one confirm.
        if (i?.data?.scheme == "ducat") claimLink.value = i.dataString
    }

    // The chosen language is applied here, before any resource is read, so a
    // screen never renders in the wrong language and corrects itself. Changing
    // it in Settings calls recreate(), which runs this again.
    override fun attachBaseContext(newBase: android.content.Context) {
        super.attachBaseContext(LocaleWrapper.wrap(newBase))
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        readIntent(intent)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        // Before super, so the splash is installed for this start rather than
        // the next one.
        installSplashScreen()
        super.onCreate(savedInstanceState)
        // Before any screen reads a contact's name. attachBaseContext has
        // already applied the chosen language, and a language change recreates
        // this activity, so the placeholder follows it.
        ContactNaming.unnamed = getString(R.string.contact_unnamed)
        // The half of DeviceLock that knows what an Activity is. Installed
        // here because the shared sources cannot name it — see DeviceLock.
        DeviceLock.backend = org.ducatproject.ducat.platform.DeviceLockAndroid
        readIntent(intent)
        val prefs = ThemePreference(this)
        setContent {
            // Follows the system unless the user has said otherwise (Menu).
            var mode by remember { mutableStateOf(prefs.mode) }
            var onboarded by remember { mutableStateOf(prefs.onboarded) }
            // Resume where a rotation or a killed process left off: the wallet
            // is persisted the moment it is created, so its presence means the
            // expensive steps are already done. Starting fresh here is what
            // used to regenerate the wallet.
            //
            // "Only the backup remains" was not quite true, and the gap it
            // left is the one thing here that a later screen cannot make up
            // for. Setup runs wallet, then PIN, then backup — so a process
            // killed between the wallet and the PIN resumed at the backup
            // step, walked to Done, and left a funded wallet with no PIN on
            // it, for ever. Ask what is actually missing instead of assuming.
            var setup by remember {
                mutableStateOf(
                    when {
                        WalletStore(this@MainActivity).address() == null -> Onboarding()
                        !Pin.isSet(this@MainActivity) -> Onboarding(step = Step.Pin)
                        else -> Onboarding(step = Step.Backup)
                    }
                )
            }

            DucatTheme(mode) {
                if (!onboarded) {
                    // Nothing behind this until it is done. §4.3's backup step is
                    // worthless if the user can reach a funded wallet without it.
                    OnboardingFlow(setup) { next ->
                        setup = next
                        if (next.step == Step.Done && next.backupConfirmed) {
                            // The persona and wallet were persisted at creation
                            // (so a rotation could not lose or regenerate them);
                            // only the profile choices, which are settings and
                            // not keys, are committed here — and the gate flag,
                            // which is what actually opens the funded wallet.
                            next.displayName?.let {
                                NameStore(this@MainActivity).put(it)
                            }
                            ContactStore(this@MainActivity)
                                .setPublishAddress(next.publishPayto)
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
    /** [jumpToBackup] lands on the backup card rather than the top of a long
     *  settings screen — the nudge that sent you there was about backups. */
    data class Drawer(val section: Section, val jumpToBackup: Boolean = false) : Overlay
}

enum class Tab(val labelRes: Int) {
    Home(R.string.tab_home),
    Accounts(R.string.tab_accounts),
    Activity(R.string.tab_activity),
    Chat(R.string.tab_chat),
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DucatApp(themeMode: ThemeMode, onThemeChange: (ThemeMode) -> Unit) {
    // Survive a rotation (and process death): the tab you were on, an open
    // Send sheet with the address it was aimed at, an open QR sheet, and the
    // overlay — the conversation you were reading, the drawer section you had
    // open. Before this, a rotation mid-payment dropped you back to Home with
    // the sheet gone.
    val context = LocalContext.current
    var tab by rememberSaveable(
        stateSaver = Saver(save = { it.name }, restore = { Tab.valueOf(it) }),
    ) { mutableStateOf(Tab.Home) }
    // The Chat case holds a Contact, which does not fit a Bundle; its persona
    // hex does, and the store re-resolves it on restore. A contact deleted
    // while the process was dead restores to None rather than a dead chat.
    var overlay by rememberSaveable(
        stateSaver = Saver<Overlay, String>(
            save = {
                when (it) {
                    is Overlay.None -> ""
                    is Overlay.Chat -> "chat:${it.contact.personaHex}"
                    is Overlay.Drawer -> "drawer:${it.section.name}"
                }
            },
            restore = { s ->
                when {
                    s.startsWith("chat:") -> ContactStore(context).all()
                        .firstOrNull { it.personaHex == s.removePrefix("chat:") }
                        ?.let { Overlay.Chat(it) } ?: Overlay.None
                    s.startsWith("drawer:") -> runCatching {
                        Overlay.Drawer(Section.valueOf(s.removePrefix("drawer:")))
                    }.getOrDefault(Overlay.None)
                    else -> Overlay.None
                }
            },
        ),
    ) { mutableStateOf<Overlay>(Overlay.None) }
    var payOpen by rememberSaveable { mutableStateOf(false) }
    var payAddress by rememberSaveable { mutableStateOf<String?>(null) }
    var qrOpen by rememberSaveable { mutableStateOf(false) }
    val drawer = rememberDrawerState(DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    val persona = remember { PersonaStore(context).secret() }

    // Android 13+ gates notifications behind a runtime ask. Once, up front:
    // this app's whole point includes "your phone tells you when money moves",
    // and a silent decline leaves it looking broken rather than muted.
    val askNotify = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.RequestPermission()
    ) { }
    LaunchedEffect(Unit) {
        if (android.os.Build.VERSION.SDK_INT >= 33 &&
            context.checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) !=
            android.content.pm.PackageManager.PERMISSION_GRANTED
        ) askNotify.launch(android.Manifest.permission.POST_NOTIFICATIONS)
    }

    // A notification names a thread; landing on Home instead is a small
    // betrayal every time. Consumed here so both cold start and warm resume
    // arrive in the same place.
    val wanted by MainActivity.openChat.collectAsState()
    LaunchedEffect(wanted) {
        val hex = wanted ?: return@LaunchedEffect
        MainActivity.openChat.value = null
        ContactStore(context).all().firstOrNull { it.personaHex == hex }
            ?.let { overlay = Overlay.Chat(it) }
    }

    // A tapped ducat: link (a chat app, an email, an NDEF sticker's browser
    // fallback). Unlike a scan — aimed at a code in front of you — a link can
    // be sent by anyone, so the card is named before it becomes a contact.
    // The name shown is the card's own claim (§16.9), like every name here.
    val tappedCard by MainActivity.claimLink.collectAsState()
    var cardAsk by remember { mutableStateOf<Pair<String, String>?>(null) }
    var cardFail by remember { mutableStateOf<Int?>(null) }
    LaunchedEffect(tappedCard) {
        val uri = tappedCard ?: return@LaunchedEffect
        // Cleared at the end, not here: this effect is keyed on the flow, so
        // emptying it first changes the key and cancels the read below at its
        // first suspension point. Reading a card is fast enough to usually win
        // that race, which is the worst kind of bug to leave lying around.
        kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
            runCatching { uniffi.ducat_mobile.readContactCard(uri) }
        }.onSuccess { card ->
            if (card.expired) cardFail = R.string.main_card_link_expired
            else cardAsk = uri to card.assertedName.orEmpty()
        }.onFailure {
            DucatLog.w("Main", "card link unreadable: ${it.message}")
            cardFail = org.ducatproject.ducat.ui.claimFailureRes(it)
        }
        MainActivity.claimLink.value = null
    }
    cardAsk?.let { (uri, who) ->
        // Their card's claim about itself, which the reader may replace before
        // adding them. A card cut before its owner set a name carries none, and
        // that is how a contact ends up called "Unnamed contact" forever: this
        // is the one moment the person is standing right there to be asked
        // about. Prefilled with what the card says, so the common case is one
        // tap and nothing to read.
        var naming by remember(uri) { mutableStateOf(who) }
        // The other half of the same question, asked in the same breath.
        //
        // This dialog exists because claiming a card is the one moment the
        // other person is standing right there to be named. They are equally
        // there to be *introduced to*, and a phone with no display name
        // asserts none on the handshake — so without this the reader labels
        // their new contact carefully and lands on that contact's screen as
        // "Unnamed contact". A second modal stacked on this one would be a
        // worse way to ask than a second field.
        val needMine = remember(uri) { org.ducatproject.ducat.ui.nameGateNeeded(context) }
        var mine by remember(uri) { mutableStateOf("") }
        AlertDialog(
            onDismissRequest = { cardAsk = null },
            title = { Text(androidx.compose.ui.res.stringResource(R.string.main_card_link_title)) },
            text = {
                Column {
                    Text(
                        androidx.compose.ui.res.stringResource(
                            if (who.isBlank()) R.string.main_card_link_body_unnamed
                            else R.string.main_card_link_body,
                            who,
                        ),
                    )
                    Spacer(Modifier.height(12.dp))
                    OutlinedTextField(
                        value = naming,
                        onValueChange = { if (it.length <= 32) naming = it },
                        label = {
                            Text(androidx.compose.ui.res.stringResource(R.string.main_card_link_name))
                        },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    if (needMine) {
                        Spacer(Modifier.height(16.dp))
                        Text(
                            androidx.compose.ui.res.stringResource(R.string.name_gate_body),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Spacer(Modifier.height(8.dp))
                        OutlinedTextField(
                            value = mine,
                            onValueChange = { if (it.length <= 32) mine = it },
                            label = {
                                Text(
                                    androidx.compose.ui.res
                                        .stringResource(R.string.name_gate_label),
                                )
                            },
                            singleLine = true,
                            modifier = Modifier.fillMaxWidth(),
                        )
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    val petname = naming.trim().takeIf { it.isNotBlank() && it != who }
                    // Before the claim, not after: claimCard reads the name
                    // store to decide what to assert about us, and a write
                    // that landed afterwards would miss this very handshake.
                    if (needMine) {
                        val store = NameStore(context)
                        mine.trim().takeIf { it.isNotBlank() }?.let { store.put(it) }
                        store.markAsked()
                    }
                    cardAsk = null
                    scope.launch {
                        kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                            runCatching {
                                val card = uniffi.ducat_mobile.readContactCard(uri)
                                Mailbox.claimCard(context, card, petname)
                            }
                        }.onSuccess { overlay = Overlay.Chat(it) }
                            .onFailure {
                                DucatLog.w("Main", "card link claim: ${it.message}")
                                // A node that has not finished connecting is
                                // not a bad card. Saying "broken, already
                                // claimed, or no longer valid" over a claim
                                // that failed offline sends someone back to ask
                                // for a replacement — burning the good card
                                // they are holding, since a card is claim-once.
                                cardFail = org.ducatproject.ducat.ui
                                    .claimFailureRes(it)
                            }
                    }
                }) { Text(androidx.compose.ui.res.stringResource(R.string.main_card_link_add)) }
            },
            dismissButton = {
                TextButton(onClick = { cardAsk = null }) {
                    Text(androidx.compose.ui.res.stringResource(R.string.main_card_link_not_now))
                }
            },
        )
    }
    cardFail?.let { why ->
        AlertDialog(
            onDismissRequest = { cardFail = null },
            confirmButton = {
                TextButton(onClick = { cardFail = null }) {
                    Text(androidx.compose.ui.res.stringResource(R.string.main_card_link_ok))
                }
            },
            title = {
                Text(androidx.compose.ui.res.stringResource(R.string.main_card_link_failed_title))
            },
            text = { Text(androidx.compose.ui.res.stringResource(why)) },
        )
    }

    // The mode owns the whole scaffold (§15.11): a till is a different app
    // from a wallet, and the drawer is the one shared door between them.
    val modeV by ContactStore.changes.collectAsState()
    val appMode = remember(modeV) { ModeStore(context).current() }

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
    // Only while the tabs are actually on screen: under a mode shell this
    // would swallow the first back press to move a tab nobody can see.
    BackHandler(
        enabled = overlay is Overlay.None && drawer.isClosed && tab != Tab.Home &&
            appMode == Mode.None,
    ) {
        tab = Tab.Home
    }

    // A bill interrupts. When a fresh PAYMENT_REQUEST lands — taxi fare, till
    // total, a friend's ask — it takes the screen the way an incoming call
    // does, whatever mode the device is in. Once per bill: dismissing leaves
    // the bubble in the chat as the paper trail, and nothing re-nags.
    var billPrompt by remember {
        mutableStateOf<Pair<Contact, StoredMessage>?>(null)
    }
    var billPay by remember { mutableStateOf<Pair<Contact, Long>?>(null) }
    val billPrefs = remember {
        securePrefs(context, "ducat_contacts")
    }
    val billV by ContactStore.changes.collectAsState()
    // Keyed on the flows a bill can be behind, not just the store: one
    // takeover at a time, so a bill arriving while another bill's prompt or
    // payment screen is up waits unseen — and the re-key runs this again the
    // moment the current flow closes, which is when the queued one appears.
    LaunchedEffect(billV, billPrompt, billPay, payOpen) {
        if (billPrompt != null || billPay != null || payOpen) return@LaunchedEffect
        val store = ContactStore(context)
        val now = System.currentTimeMillis() / 1000
        for (c in store.all()) {
            val m = store.thread(c.personaHex)
                .lastOrNull { !it.outgoing && it.kind == 1 } ?: continue
            val seen = billPrefs.getLong("billseen_${c.personaHex}", -1L)
            // Fresh only: reinstalls and restores must not replay history as
            // a stack of surprise take-overs.
            if (m.seq > seen && now - m.timestamp < 300) {
                billPrompt = c to m
                break
            }
        }
    }
    billPrompt?.let { (c, m) ->
        val markSeen = {
            billPrefs.edit().putLong("billseen_${c.personaHex}", m.seq).apply()
            billPrompt = null
        }
        org.ducatproject.ducat.ui.BillScreen(
            m = m,
            contact = c,
            onPay = { markSeen(); billPay = c to m.amountPxmr },
            onDecline = {
                markSeen()
                val mine = PersonaStore(context).personaHex()
                val body = context.getString(
                    R.string.main_bill_decline,
                    Amounts.show(context, m.amountPxmr).primary,
                )
                // Advisory, but not fire-and-forget: the other side is waiting
                // on this bill, and a decline that never leaves reads as being
                // ignored. One retry, then a log line so field logs show the
                // decline never went out.
                kotlinx.coroutines.MainScope().launch(kotlinx.coroutines.Dispatchers.IO) {
                    var r = runCatching { Mailbox.send(context, c, body, mine) }
                    if (r.isFailure) {
                        kotlinx.coroutines.delay(5_000)
                        r = runCatching { Mailbox.send(context, c, body, mine) }
                    }
                    r.onFailure {
                        DucatLog.w(
                            "Bill",
                            "decline for ${formatXmr(m.amountPxmr)} XMR to " +
                                "${c.displayName()} never sent: ${it.message}",
                        )
                    }
                }
            },
            onClose = markSeen,
        )
    }
    billPay?.let { (c, amt) ->
        PaySheet(prefillContact = c, prefillAmountPxmr = amt) { billPay = null }
    }

    // Picking a mode should land you *in* it, not leave the picker on top.
    LaunchedEffect(appMode) {
        (overlay as? Overlay.Drawer)?.let {
            if (it.section == Section.Modes) overlay = Overlay.None
        }
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
                            title = {
                                Text(androidx.compose.ui.res.stringResource(o.section.labelRes))
                            },
                            navigationIcon = {
                                IconButton(onClick = { overlay = Overlay.None }) {
                                    Icon(
                                        Icons.Filled.ArrowBack,
                                        contentDescription = androidx.compose.ui.res
                                            .stringResource(R.string.main_back),
                                    )
                                }
                            },
                        )
                    },
                ) { padding ->
                    Box(Modifier.padding(padding)) {
                        SectionScreen(
                            o.section, themeMode, onThemeChange,
                            jumpToBackup = o.jumpToBackup,
                        ) {
                            overlay = Overlay.Chat(it)
                        }
                    }
                }
                return@ModalNavigationDrawer
            }
            Overlay.None -> {}
        }

        // A job takes the whole screen. Overlays (chat, drawer sections) have
        // already returned above, so a notification can still drop the till
        // into the conversation it names and back lands in the shell.
        if (appMode != Mode.None) {
            org.ducatproject.ducat.ui.ModeShell(appMode) { scope.launch { drawer.open() } }
            return@ModalNavigationDrawer
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
                            Text(androidx.compose.ui.res.stringResource(tab.labelRes),
                                style = MaterialTheme.typography.titleLarge)
                        }
                    },
                    navigationIcon = {
                        IconButton(onClick = { scope.launch { drawer.open() } }) {
                            Icon(Icons.Filled.Menu, contentDescription =
                                androidx.compose.ui.res.stringResource(R.string.main_menu))
                        }
                    },
                    actions = {
                        IconButton(onClick = { qrOpen = true }) {
                            Icon(Icons.Filled.QrCode2, contentDescription =
                                androidx.compose.ui.res.stringResource(R.string.main_codes))
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
                            // the circle floats in the wrapper above. A fixed
                            // height, never fillMaxHeight — NavigationBar does
                            // not constrain its children, so filling it expands
                            // the bar to the whole screen. That lesson was in a
                            // comment here once, which got deleted with the code
                            // it annotated and promptly needed relearning.
                            Box(
                                Modifier.weight(1f).height(80.dp),
                                contentAlignment = Alignment.BottomCenter,
                            ) {
                                Text(
                                    androidx.compose.ui.res.stringResource(
                                        R.string.tab_send_receive),
                                    style = MaterialTheme.typography.labelSmall,
                                    maxLines = 1,
                                    softWrap = false,
                                    modifier = Modifier.padding(bottom = 14.dp),
                                )
                            }
                            NavItem(Tab.Activity, Icons.Filled.Receipt, tab) { tab = it }
                            // The one number a messenger owes its bottom bar:
                            // how many conversations are waiting.
                            val unv by ContactStore.changes.collectAsState()
                            val unread = remember(unv) { ContactStore(context).unreadThreads() }
                            NavigationBarItem(
                                selected = tab == Tab.Chat,
                                onClick = { tab = Tab.Chat },
                                icon = {
                                    BadgedBox(badge = {
                                        if (unread > 0) Badge { Text("$unread") }
                                    }) {
                                        Icon(Icons.Filled.ChatBubble, contentDescription =
                                            androidx.compose.ui.res.stringResource(Tab.Chat.labelRes))
                                    }
                                },
                                label = {
                                    Text(androidx.compose.ui.res.stringResource(Tab.Chat.labelRes))
                                },
                            )
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
                            //
                            // Its own PNG, **not** R.mipmap.ic_launcher: on any
                            // modern device that name resolves to the adaptive
                            // <adaptive-icon> XML, which painterResource cannot
                            // load — it throws at first composition, which is
                            // the app crashing on open.
                            androidx.compose.foundation.Image(
                                painterResource(R.drawable.ducat_cat),
                                contentDescription = androidx.compose.ui.res
                                    .stringResource(R.string.main_send_or_receive),
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
                    //
                    // In POS mode the Home tab *is* the till. A mode is a
                    // stance, not a feature: the person behind a counter rings
                    // up sale after sale, and making them navigate to it before
                    // every customer is making them do it forty times a shift.
                    Tab.Home -> Column(Modifier.verticalScroll(rememberScrollState())) {
                        HomeScreen(
                            onTopUp = { tab = Tab.Accounts },
                            onSeeActivity = { tab = Tab.Activity },
                            onBackup = {
                                overlay = Overlay.Drawer(
                                    Section.Settings, jumpToBackup = true,
                                )
                            },
                            onOpenChat = { overlay = Overlay.Chat(it) },
                        )
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
private fun HomeScreen(
    onTopUp: () -> Unit,
    onSeeActivity: () -> Unit,
    onBackup: () -> Unit,
    onOpenChat: (Contact) -> Unit,
) {
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

    // The three jobs that belong on the personal screen, as one row of
    // squares. Hailing is a rider's moment rather than an operating mode, and
    // looking for a car or a place is the same shape of moment — things a
    // person does occasionally, not jobs they run. The modes are for whoever
    // *has* a car or a room to let (§16.18).
    val hailSheet = remember { mutableStateOf(false) }
    val rentKind = remember { mutableStateOf<Int?>(null) }
    Spacer(Modifier.height(12.dp))
    org.ducatproject.ducat.ui.HomeTiles(
        onHail = { hailSheet.value = true },
        onBrowse = { rentKind.value = it },
    )
    // Both flows live on, tiles or not: the hail keeps its card for a hail
    // that is actually standing, and each owns its own screens.
    org.ducatproject.ducat.ui.HailCard(sheetState = hailSheet)
    org.ducatproject.ducat.ui.RentSearchCard(onOpenChat = onOpenChat, kindState = rentKind)

    // The nudge that keeps §4.3 true: the bundle carries the relationships
    // now, so every contact made after the last export is one a restore will
    // not bring back — and nobody re-exports unprompted.
    val stale = remember(version) { ContactStore(context).backupStale() }
    if (stale) {
        Spacer(Modifier.height(12.dp))
        Surface(
            color = MaterialTheme.colorScheme.surfaceVariant,
            shape = MaterialTheme.shapes.large,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        ) {
            Row(
                Modifier.clickable { onBackup() }.padding(14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(
                        androidx.compose.ui.res.stringResource(R.string.main_backup_title),
                        style = MaterialTheme.typography.titleSmall,
                    )
                    Text(
                        androidx.compose.ui.res.stringResource(R.string.main_backup_body),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Spacer(Modifier.width(8.dp))
                Icon(
                    Icons.Filled.ChevronRight,
                    contentDescription = androidx.compose.ui.res
                        .stringResource(R.string.main_open_backup_settings),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }

    // The last few movements, right under the number they explain — the shape
    // every payments app the user knows leads with. Three rows, then the tab.
    val recent = remember(version) { Ledger.build(context).take(3) }
    if (recent.isNotEmpty()) {
        Spacer(Modifier.height(16.dp))
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 24.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                androidx.compose.ui.res.stringResource(R.string.main_recent),
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.weight(1f))
            TextButton(onClick = onSeeActivity) {
                Text(androidx.compose.ui.res.stringResource(R.string.main_see_all))
            }
        }
        recent.forEach { e ->
            val sent = e.direction == Ledger.Direction.Sent
            val shown = Amounts.show(context, e.amountPxmr)
            Row(
                Modifier.fillMaxWidth()
                    .padding(horizontal = 24.dp, vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(
                    if (sent) Icons.Filled.ArrowUpward else Icons.Filled.ArrowDownward,
                    null,
                    Modifier.size(18.dp),
                    tint = if (sent) MaterialTheme.ducat.changePending
                    else MaterialTheme.ducat.settled,
                )
                Spacer(Modifier.width(10.dp))
                Column(Modifier.weight(1f)) {
                    Text(
                        e.counterparty ?: androidx.compose.ui.res.stringResource(
                            if (sent) R.string.main_sent else R.string.main_received),
                        style = MaterialTheme.typography.bodyMedium,
                        maxLines = 1,
                        overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                    )
                    Text(
                        org.ducatproject.ducat.ui.shortWhen(context, e.timestamp),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.outline,
                    )
                }
                Text(
                    "${if (sent) "−" else "+"}${shown.primary}",
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }
        Spacer(Modifier.height(8.dp))
    }
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
        icon = {
            Icon(icon, contentDescription =
                androidx.compose.ui.res.stringResource(target.labelRes))
        },
        label = { Text(androidx.compose.ui.res.stringResource(target.labelRes)) },
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
