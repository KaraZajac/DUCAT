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
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
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
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
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

        /** The group a notification named ("Sam · ladder crew"). Its own
         *  channel because the sender's thread does not show what was
         *  announced: a group's rows live in the members' pairwise logs and
         *  the pairwise screen keeps them out, so opening Sam on a tap that
         *  promised the ladder crew landed on nothing new. */
        val openGroup = kotlinx.coroutines.flow.MutableStateFlow<String?>(null)

        /**
         * Which noun Renting's board should open on.
         *
         * Three home tiles lead into one mode — a room, a car, a kayak — and
         * the mode has to be told which door was used. Kept rather than
         * cleared after reading: it is the last thing asked for, and re-entering
         * the mode from the drawer should land where it was left.
         */
        val browseKind = kotlinx.coroutines.flow.MutableStateFlow<Int?>(null)

        /** A tapped ducat: link, waiting for the shell to show who it is. */
        val claimLink = kotlinx.coroutines.flow.MutableStateFlow<String?>(null)

        /** "List yours" from an empty market shelf: open the press room. */
        val openPublishing = kotlinx.coroutines.flow.MutableStateFlow(false)

        /** A ducat:site/ link asking for the Sites shelf. */
        val openSites = kotlinx.coroutines.flow.MutableStateFlow(false)
    }

    private fun readIntent(i: android.content.Intent?) {
        i?.getStringExtra("open_group")?.let { openGroup.value = it }
            ?: i?.getStringExtra("open_chat")?.let { openChat.value = it }
        // §18.7 token mode: the manifest registers ducat: links, and this URI
        // used to stop right here, read by nobody — a tapped card opened the
        // app to Home, silently. It now reaches the same claim the scanner
        // runs, behind one confirm.
        if (i?.data?.scheme == "ducat") {
            val uri = i.dataString
            // §16.22 addresses route to the Sites shelf; everything else is
            // a card and takes the claim road it always has.
            if (uri != null && uri.startsWith("ducat:site/")) {
                org.ducatproject.ducat.ui.pendingSiteAdd.value = uri
                openSites.value = true
            } else {
                claimLink.value = uri
            }
        }
    }

    // The chosen language is applied here, before any resource is read, so a
    // screen never renders in the wrong language and corrects itself. Changing
    // it in Settings calls recreate(), which runs this again.
    override fun attachBaseContext(newBase: android.content.Context) {
        super.attachBaseContext(LocaleWrapper.wrap(newBase))
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        // Recorded as the current one: a recreate after this would otherwise
        // read the *launch* intent back, not the latest.
        setIntent(intent)
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
        // And the notification channels' names on the phone's Settings page,
        // for the same reason.
        Notify.refreshChannels(this)
        // The half of DeviceLock that knows what an Activity is. Installed
        // here because the shared sources cannot name it — see DeviceLock.
        DeviceLock.backend = org.ducatproject.ducat.platform.DeviceLockAndroid
        // §16.21's ears and mouth, same pattern for the same reason.
        Calls.audio = org.ducatproject.ducat.platform.CallAudioAndroid
        // A worldwide-market Subscribe is an ordinary card claim: hand the
        // row's ducat: URI to the same sheet a scanned code opens.
        // §16.22: the sealed-room viewer is an Activity; the shared screen
        // only holds the hook.
        org.ducatproject.ducat.ui.siteOpen = { ctx, recordKey ->
            ctx.startActivity(
                android.content.Intent(ctx, SiteViewerActivity::class.java)
                    .putExtra("record", recordKey),
            )
        }
        // The answering machine's road home: the leave-a-message button
        // opens the thread whose mic is the recorder.
        org.ducatproject.ducat.ui.callOpenThread = { hex -> openChat.value = hex }
        org.ducatproject.ducat.ui.marketSubscribe = { claimLink.value = it }
        // The Library's Open: view first, share sheet when nothing on the
        // device claims the type. Lives here because FileProvider and the
        // chooser are Android; the shared screen only holds the hook.
        // Counter etiquette: full brightness while any QR is on screen, put
        // back the moment the last one leaves. Refcounted — two codes can
        // overlap during a transition and the first to close must not dim
        // the one still up.
        var qrsUp = 0
        org.ducatproject.ducat.ui.qrLit = { on ->
            runOnUiThread {
                qrsUp = (qrsUp + if (on) 1 else -1).coerceAtLeast(0)
                val lp = window.attributes
                lp.screenBrightness = if (qrsUp > 0) {
                    1f
                } else {
                    android.view.WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE
                }
                window.attributes = lp
            }
        }
        org.ducatproject.ducat.ui.libraryOpen = { ctx, publisherHex, period ->
            val dir = java.io.File(ctx.filesDir, "publications/$publisherHex/$period")
            val file = dir.walkTopDown()
                .filter { it.isFile && !it.name.endsWith(".part") }
                .maxByOrNull { it.length() }
            if (file != null) {
                val uri = androidx.core.content.FileProvider.getUriForFile(
                    ctx, "${ctx.packageName}.backups", file,
                )
                val mime = android.webkit.MimeTypeMap.getSingleton()
                    .getMimeTypeFromExtension(file.extension.lowercase()) ?: "*/*"
                val view = android.content.Intent(android.content.Intent.ACTION_VIEW).apply {
                    setDataAndType(uri, mime)
                    addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
                runCatching { ctx.startActivity(view) }.onFailure {
                    val send = android.content.Intent(android.content.Intent.ACTION_SEND).apply {
                        type = mime
                        putExtra(android.content.Intent.EXTRA_STREAM, uri)
                        addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION)
                    }
                    runCatching {
                        ctx.startActivity(
                            android.content.Intent.createChooser(send, file.name),
                        )
                    }
                }
            }
        }
        org.ducatproject.ducat.ui.marketListYours = { openPublishing.value = true }
        Calls.shell = object : Calls.Shell {
            override fun takeover(context: android.content.Context, from: String) {
                // A lit, watched app already shows CallScreen by itself.
                if (!AppVisibility.foreground) Notify.ringIncoming(context, from)
            }
            override fun release(context: android.content.Context) {
                Notify.quietIncoming(context)
            }
            // The node's own service, wearing the microphone type for the
            // length of the call — see NodeService.inCall.
            override fun connected(context: android.content.Context, from: String) {
                NodeService.inCall(context, from)
            }
            override fun calling(context: android.content.Context, to: String) {
                NodeService.inCall(context, to, ringing = true)
            }
            override fun ended(context: android.content.Context) {
                NodeService.callEnded(context)
            }
        }
        // First creation only. A rotation or a language change recreates the
        // activity with the same intent, and reading it again re-opened the
        // tapped card's confirm — or the notification's thread — on top of
        // wherever the person had since gone.
        if (savedInstanceState == null) readIntent(intent)
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
                        // A restore lands here too — wallet in place, PIN
                        // still to choose — and it has already recorded the
                        // backup that got it here. Resuming with the defaults
                        // marched a restored phone through Trust and Backup
                        // again, and Done then wrote the default "publish my
                        // address" over the choice the backup carried.
                        !Pin.isSet(this@MainActivity) -> {
                            val store = ContactStore(this@MainActivity)
                            val restored = store.backupExportedAt() > 0L
                            Onboarding(
                                step = Step.Pin,
                                backupConfirmed = restored,
                                publishPayto = if (restored) {
                                    store.publishAddress()
                                } else {
                                    Onboarding().publishPayto
                                },
                            )
                        }
                        // Ask about the backup too, rather than assuming it is
                        // the thing still outstanding. A restore records one —
                        // the file that got them here — and so does making one
                        // at the last step, so this is answerable from the same
                        // durable state the two checks above use.
                        //
                        // Assuming it left a restored phone being marched
                        // through Trust and Backup to produce a bundle it did
                        // not need, and could not leave setup until it did.
                        // Whether that happened at all came down to whether the
                        // process had been killed since the restore, which is
                        // not something onboarding should vary on.
                        ContactStore(this@MainActivity).backupExportedAt() > 0L ->
                            Onboarding(
                                step = Step.Done, backupConfirmed = true,
                                // Finish writes this back; carry the stored
                                // choice rather than the default.
                                publishPayto = ContactStore(this@MainActivity)
                                    .publishAddress(),
                            )
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
    /** A group's conversation (§16.19). An overlay like a pairwise one: it
     *  used to be drawn inside the Chats tab, under the tab's own bar — two
     *  headers stacked on a chat, and the back gesture, which the tab shell
     *  owns, left it for Home rather than for the list it came from. */
    data class Group(val idHex: String) : Overlay
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
                    is Overlay.Group -> "group:${it.idHex}"
                    is Overlay.Drawer -> "drawer:${it.section.name}"
                }
            },
            restore = { s ->
                when {
                    s.startsWith("chat:") -> ContactStore(context).all()
                        .firstOrNull { it.personaHex == s.removePrefix("chat:") }
                        ?.let { Overlay.Chat(it) } ?: Overlay.None
                    s.startsWith("group:") -> s.removePrefix("group:")
                        .takeIf { Groups.get(context, it) != null }
                        ?.let { Overlay.Group(it) } ?: Overlay.None
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
    // What the scanned code asked for, when it asked for anything.
    var payAmountPxmr by rememberSaveable { mutableStateOf(0L) }
    var qrOpen by rememberSaveable { mutableStateOf(false) }
    val drawer = rememberDrawerState(DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    val persona = remember { PersonaStore(context).secret() }

    // The mode owns the whole scaffold (§15.11): a till is a different app
    // from a wallet, and the drawer is the one shared door between them.
    val modeV by ContactStore.changes.collectAsState()
    val appMode = remember(modeV) { ModeStore(context).current() }
    // A kiosk is an unattended device in a shop, and its one way out is the
    // staff door behind the PIN. So nothing else that can reach this screen
    // — the drawer's edge swipe, a tapped link, a notification's thread, a
    // ring, a bill — may take the surface from it. Each is answered below
    // the way an empty shop answers it, rather than held for whoever next
    // types the PIN.
    val kiosk = appMode == Mode.Kiosk

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
        if (kiosk) return@LaunchedEffect
        ContactStore(context).all().firstOrNull { it.personaHex == hex }
            ?.let { overlay = Overlay.Chat(it) }
    }
    val wantedGroup by MainActivity.openGroup.collectAsState()
    LaunchedEffect(wantedGroup) {
        val hex = wantedGroup ?: return@LaunchedEffect
        MainActivity.openGroup.value = null
        if (kiosk) return@LaunchedEffect
        if (Groups.get(context, hex) != null) overlay = Overlay.Group(hex)
    }

    // A tapped ducat: link (a chat app, an email, an NDEF sticker's browser
    // fallback). Unlike a scan — aimed at a code in front of you — a link can
    // be sent by anyone, so the card is named before it becomes a contact.
    // The name shown is the card's own claim (§16.9), like every name here.
    val wantPublishing by MainActivity.openPublishing.collectAsState()
    LaunchedEffect(wantPublishing) {
        if (wantPublishing) {
            if (!kiosk) overlay = Overlay.Drawer(Section.Publishing)
            MainActivity.openPublishing.value = false
        }
    }
    val wantSites by MainActivity.openSites.collectAsState()
    LaunchedEffect(wantSites) {
        if (wantSites) {
            if (kiosk) {
                // The address the link carried is waiting on the section
                // it will never reach; drop it with the request.
                org.ducatproject.ducat.ui.pendingSiteAdd.value = null
            } else {
                overlay = Overlay.Drawer(Section.Sites)
            }
            MainActivity.openSites.value = false
        }
    }
    val tappedCard by MainActivity.claimLink.collectAsState()
    var cardAsk by remember { mutableStateOf<Pair<String, String>?>(null) }
    // Not a new person. A card naming somebody already in the list is the
    // ordinary way a contact comes back after losing their phone — and it is
    // also how an attacker reaches an existing record, since a card carries a
    // persona with nothing signed over it. Either way "Add Sam?" is the wrong
    // question, so the dialog asks the right one.
    var cardKnown by remember { mutableStateOf<Contact?>(null) }
    var cardFail by remember { mutableStateOf<Int?>(null) }
    LaunchedEffect(tappedCard) {
        val uri = tappedCard ?: return@LaunchedEffect
        if (kiosk) {
            MainActivity.claimLink.value = null
            return@LaunchedEffect
        }
        // Cleared at the end, not here: this effect is keyed on the flow, so
        // emptying it first changes the key and cancels the read below at its
        // first suspension point. Reading a card is fast enough to usually win
        // that race, which is the worst kind of bug to leave lying around.
        kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
            runCatching { uniffi.ducat_mobile.readContactCard(uri) }
        }.onSuccess { card ->
            if (card.expired) cardFail = R.string.main_card_link_expired
            else cardAsk = uri to card.assertedName.orEmpty()
            cardKnown = runCatching {
                val hex = card.persona.joinToString("") { "%02x".format(it) }
                ContactStore(context).all().firstOrNull { it.personaHex == hex }
            }.getOrNull()
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
                    cardKnown?.let { known ->
                        Spacer(Modifier.height(12.dp))
                        Text(
                            androidx.compose.ui.res.stringResource(
                                R.string.main_card_link_known,
                                org.ducatproject.ducat.ui.isolate(known.displayName()),
                            ),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.error,
                        )
                    }
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
                    cardKnown = null
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
                TextButton(onClick = { cardAsk = null; cardKnown = null }) {
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

    // §16.21: a live call outranks every other screen — a telephone that
    // hides its own call behind a till is not a telephone. The observer
    // rides the store's own change signal, so a ring lands the moment the
    // poller files it.
    val callV by ContactStore.changes.collectAsState()
    // Off the main thread: noticing means reading every thread in the store
    // on every change to it, and the poller already calls this from its own.
    LaunchedEffect(callV) {
        kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
            Calls.noticed(context)
        }
    }
    // While a call exists, this activity may be woken by the full-screen
    // ask on a dark, locked phone — it must show over the keyguard and
    // light the screen, and must stop doing either the moment the call
    // ends: these flags on an idle wallet would put balances above locks.
    val callState = Calls.state
    val inCall = callState != Calls.State.Idle
    LaunchedEffect(inCall, kiosk) {
        (context as? android.app.Activity)?.let {
            if (android.os.Build.VERSION.SDK_INT >= 27) {
                it.setShowWhenLocked(inCall && !kiosk)
                it.setTurnScreenOn(inCall && !kiosk)
            }
            // A telephone does not go dark mid-sentence: the screen stays
            // on for the call and goes back to its timeout after it.
            val keepOn = android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON
            if (inCall && !kiosk) it.window.addFlags(keepOn) else it.window.clearFlags(keepOn)
        }
    }
    // A kiosk has nobody to pick up. The ring is declined — the caller
    // hears the till's own word for "no", not a phone that rings out —
    // and the answering-machine screen it would leave behind is dismissed,
    // because the call screen is the one door here with no PIN on it.
    LaunchedEffect(callState, kiosk) {
        if (!kiosk) return@LaunchedEffect
        when (callState) {
            Calls.State.Idle -> {}
            is Calls.State.NoAnswer -> Calls.dismissNoAnswer()
            is Calls.State.Incoming -> {
                val (c, offer) = kotlinx.coroutines.withContext(
                    kotlinx.coroutines.Dispatchers.IO,
                ) {
                    val store = ContactStore(context)
                    val c = store.all().firstOrNull { it.personaHex == callState.contactHex }
                    // By id as well as seq: a fresh card restarts the
                    // numbering, so one thread can hold several rows at
                    // the offer's seq, and a decline naming the wrong one
                    // withdraws nothing.
                    c to c?.let { store.thread(it.personaHex) }
                        ?.lastOrNull {
                            it.seq == callState.offerSeq && !it.outgoing &&
                                it.callId == callState.callId
                        }
                }
                if (c != null && offer != null) Calls.decline(context, c, offer) else Calls.hangUp()
            }
            else -> Calls.hangUp()
        }
    }
    if (inCall && !kiosk) {
        org.ducatproject.ducat.ui.CallScreen()
        return
    }

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
    var billPay by remember { mutableStateOf<Triple<Contact, Long, Long>?>(null) }
    val billPrefs = remember {
        securePrefs(context, "ducat_contacts")
    }
    val billV by ContactStore.changes.collectAsState()
    // Keyed on the flows a bill can be behind, not just the store: one
    // takeover at a time, so a bill arriving while another bill's prompt or
    // payment screen is up waits unseen — and the re-key runs this again the
    // moment the current flow closes, which is when the queued one appears.
    LaunchedEffect(billV, billPrompt, billPay, payOpen, kiosk) {
        if (billPrompt != null || billPay != null || payOpen) return@LaunchedEffect
        // A kiosk pays nobody: the bill stays in the thread for the staff,
        // where it would have gone anyway once dismissed, and the till's
        // wallet is not offered to whoever is standing at the counter.
        if (kiosk) return@LaunchedEffect
        // Every thread in the store, read on each change to it — off the
        // main thread, which was drawing the screen the bill takes over.
        val found = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
            val store = ContactStore(context)
            val now = System.currentTimeMillis() / 1000
            var hit: Pair<Contact, StoredMessage>? = null
            for (c in store.all()) {
                val thread = store.thread(c.personaHex)
                val m = thread.lastOrNull { !it.outgoing && it.kind == 1 } ?: continue
                // Not one that is already settled. `billdone_` only records
                // bills this prompt itself handled, so paying or declining
                // one from the thread left it unmarked — and inside the
                // five-minute freshness window the prompt then took the
                // screen over to offer a bill that had just been paid.
                if (Ledger.billAnswered(thread, m)) continue
                // The bill itself, not "anything past this number": a seq
                // is per card, and a till mints a card per sale, so a
                // repeat customer's next bill arrives numbered *below* the
                // one they paid last week — and never took the screen.
                val seen = billPrefs.getString("billdone_${c.personaHex}", null)
                // Fresh only: reinstalls and restores must not replay history
                // as a stack of surprise take-overs. Both directions — the
                // stamp is the asker's clock, and a bill stamped ahead of
                // ours (fast clock, or worse) otherwise counts as "fresh"
                // until its stamp passes, taking the screen over at every
                // launch until then.
                if (seen != "${m.seq}:${m.timestamp}" &&
                    kotlin.math.abs(now - m.timestamp) < 300
                ) {
                    hit = c to m
                    break
                }
            }
            hit
        }
        if (found != null) billPrompt = found
    }
    billPrompt?.let { (c, m) ->
        val markSeen = {
            billPrefs.edit()
                .putString("billdone_${c.personaHex}", "${m.seq}:${m.timestamp}")
                .apply()
            billPrompt = null
        }
        org.ducatproject.ducat.ui.BillScreen(
            m = m,
            contact = c,
            // The seq rides along: a payment that names its bill is the
            // whole of §16.14's attribution, and this prompt is the path
            // most payments take — dropping it here silently re-opened the
            // two-identical-bills ambiguity everywhere downstream.
            onPay = { markSeen(); billPay = Triple(c, m.amountPxmr, m.seq) },
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
                // A kind-5 Retract naming the bill, the same as the thread's
                // own Decline. As plain text this told them in words and told
                // neither client anything, so the bill stayed live on both
                // sides — still offering "Review payment" in the thread, still
                // listed under "Awaiting" — and this is the path most declines
                // take, because this prompt is what appears when a bill lands.
                val decline: () -> Result<Unit> = {
                    runCatching {
                        Mailbox.send(
                            context, c, body,
                            kind = 5, reSeq = m.seq, reOwn = false,
                        )
                        Unit
                    }
                }
                kotlinx.coroutines.MainScope().launch(kotlinx.coroutines.Dispatchers.IO) {
                    var r = decline()
                    if (r.isFailure) {
                        kotlinx.coroutines.delay(5_000)
                        r = decline()
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
    billPay?.let { (c, amt, seq) ->
        PaySheet(
            prefillContact = c, prefillAmountPxmr = amt,
            answersSeq = seq,
        ) { billPay = null }
    }

    // Picking a mode should land you *in* it, not leave the picker on top.
    LaunchedEffect(appMode) {
        (overlay as? Overlay.Drawer)?.let {
            if (it.section == Section.Modes) overlay = Overlay.None
        }
        // Nothing may be left standing over a kiosk: whatever section or
        // thread was up when the mode was chosen, and the drawer itself.
        if (appMode == Mode.Kiosk) {
            overlay = Overlay.None
            if (drawer.isOpen) drawer.close()
        }
    }

    ModalNavigationDrawer(
        drawerState = drawer,
        // The kiosk draws no menu button, but the drawer also opens on an
        // edge swipe — every section, Modes included, one gesture from the
        // counter. The drawer is a door for staff; in a kiosk the PIN is.
        gesturesEnabled = !kiosk,
        drawerContent = {
            DrawerContent { section ->
                scope.launch { drawer.close() }
                overlay = Overlay.Drawer(section)
            }
        },
    ) {
        // The screens under an overlay keep their saveable state while the
        // overlay is up, and the personal tabs keep theirs across a tab
        // switch. Both used to be rebuilt from nothing: the overlay and the
        // `when (tab)` below drop the screen out of composition, and a
        // `rememberSaveable` with no holder only ever restores from the
        // activity's bundle — a scroll position, a half-typed search, the
        // sale a till was ringing up when a notification opened a chat.
        val kept = androidx.compose.runtime.saveable.rememberSaveableStateHolder()
        when (val o = overlay) {
            is Overlay.Chat -> {
                ChatScreen(o.contact) { overlay = Overlay.None }
                return@ModalNavigationDrawer
            }
            is Overlay.Group -> {
                org.ducatproject.ducat.ui.GroupChatScreen(o.idHex) { overlay = Overlay.None }
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
                                        Icons.AutoMirrored.Filled.ArrowBack,
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
            kept.SaveableStateProvider("mode:${appMode.name}") {
                org.ducatproject.ducat.ui.ModeShell(appMode) { scope.launch { drawer.open() } }
            }
            return@ModalNavigationDrawer
        }

        // The whole send/request flow: who first, then how much, with contacts
        // listed above an address field because a payment to a contact carries
        // a note and a thread and an address payment carries neither.
        if (payOpen) PaySheet(
            prefillAddress = payAddress,
            prefillAmountPxmr = payAmountPxmr,
        ) { payOpen = false; payAddress = null; payAmountPxmr = 0L }

        // Venmo puts this in the corner of every screen, and the reason it
        // works is that "show me yours / here is mine" is one gesture between
        // two people standing together — not two features in two menus.
        if (qrOpen) {
            QrHub(
                onOpenChat = { qrOpen = false; overlay = Overlay.Chat(it) },
                onScanAddress = { addr, pxmr ->
                    qrOpen = false; payAddress = addr; payAmountPxmr = pxmr; payOpen = true
                },
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
                            // The centre slot: a real bar item with an empty
                            // icon, the circle floating over it from the
                            // wrapper above.
                            //
                            // It used to be a hand-built Box that placed its
                            // label with `padding(bottom = 14.dp)`, guessing at
                            // where Material puts the other four. It guessed
                            // wrong, and measurably: the other labels sat at
                            // y=2253 and this one at y=2266, in a smaller type
                            // besides. Thirteen pixels is not much to look at
                            // and is exactly what makes a bar look hand-made.
                            // An empty icon of the same 24.dp reserves the same
                            // space theirs does, so the label lands where
                            // theirs land because it is placed by the same
                            // code, not because the number was tuned until it
                            // matched.
                            //
                            // It also makes the whole slot tappable. Only the
                            // circle was, so a finger that landed on the word
                            // "Send/Receive" did nothing at all.
                            NavigationBarItem(
                                selected = false,
                                onClick = { payOpen = true },
                                icon = { Spacer(Modifier.size(24.dp)) },
                                label = {
                                    Text(
                                        androidx.compose.ui.res.stringResource(
                                            R.string.tab_send_receive),
                                        // The longest label in the bar, in the
                                        // narrowest fifth of it. Material's
                                        // default size does not fit at all, so
                                        // the other four match this one rather
                                        // than the other way round — see
                                        // NavItem.
                                        style = MaterialTheme.typography.labelSmall,
                                        maxLines = 1,
                                        softWrap = false,
                                        // Measured at its own width, not at the
                                        // slot's. A bar item keeps horizontal
                                        // padding for its label, which leaves
                                        // less than "Send/Receive" needs, and
                                        // `softWrap = false` spends the
                                        // shortfall on clipping the last letter
                                        // rather than on wrapping — the bar
                                        // read "Send/Receiv" with the e cut
                                        // through. Four of the nineteen
                                        // translations are longer than the
                                        // English ("Enviar/Receber",
                                        // "Wyślij/Odbierz"), so this was going
                                        // to be worse elsewhere than it looked
                                        // here. The neighbours' labels end
                                        // well short of this slot, so the few
                                        // pixels it takes back are empty ones.
                                        modifier = Modifier.wrapContentWidth(unbounded = true),
                                    )
                                },
                            )
                            NavItem(Tab.Activity, Icons.Filled.Receipt, tab) { tab = it }
                            // The one number a messenger owes its bottom bar:
                            // how many conversations are waiting.
                            val unv by ContactStore.changes.collectAsState()
                            // IO, not remember: this counts by decrypting the
                            // whole contact book, and it sat in the nav bar —
                            // the one composable on screen for every frame of
                            // the app's life.
                            val unread by produceState(0, unv) {
                                value = withContext(Dispatchers.IO) {
                                    // Groups count as their own rows now
                                    // that their traffic no longer flags
                                    // the sender's thread (Groups.markSeen).
                                    ContactStore(context).unreadThreads() +
                                        org.ducatproject.ducat.Groups.unreadGroups(context)
                                }
                            }
                            NavigationBarItem(
                                selected = tab == Tab.Chat,
                                onClick = { tab = Tab.Chat },
                                icon = {
                                    BadgedBox(badge = {
                                        if (unread > 0) Badge { Text(org.ducatproject.ducat.Amounts.count(unread.toLong())) }
                                    }) {
                                        Icon(Icons.Filled.ChatBubble, contentDescription =
                                            androidx.compose.ui.res.stringResource(Tab.Chat.labelRes))
                                    }
                                },
                                label = {
                                    // Spelled out rather than via NavItem,
                                    // because this one carries the badge — so
                                    // it needs the same label style spelled
                                    // out too, or it is the odd one left.
                                    Text(
                                        androidx.compose.ui.res.stringResource(Tab.Chat.labelRes),
                                        style = MaterialTheme.typography.labelSmall,
                                        maxLines = 1,
                                        softWrap = false,
                                    )
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
                kept.SaveableStateProvider(tab.name) {
                    when (tab) {
                        // Only the scrolling screens get a scroll wrapper. Chat
                        // owns a LazyColumn, and nesting one inside a vertical
                        // scroll gives it unbounded height — it renders every
                        // row at once and the list stops being lazy.
                        //
                        // In POS mode the Home tab *is* the till. A mode is a
                        // stance, not a feature: the person behind a counter
                        // rings up sale after sale, and making them navigate to
                        // it before every customer is making them do it forty
                        // times a shift.
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
                                onLibrary = { overlay = Overlay.Drawer(Section.Library) },
                            )
                        }
                        Tab.Accounts -> AccountsScreen()
                        Tab.Activity -> ActivityScreen()
                        Tab.Chat -> ChatListScreen(
                            persona,
                            onOpenChat = { overlay = Overlay.Chat(it) },
                            onOpenGroup = { overlay = Overlay.Group(it) },
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun HomeScreen(
    onTopUp: () -> Unit,
    onLibrary: () -> Unit,
    onSeeActivity: () -> Unit,
    onBackup: () -> Unit,
    onOpenChat: (Contact) -> Unit,
) {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    // Off the main thread: balances() decrypts every wallet output per call,
    // and the home screen re-read it in composition on every store bump. The
    // last loaded figure stays on screen while a fresh one is computed, so
    // the card never blanks — it is at most one bump stale for a moment.
    var loaded by remember { mutableStateOf<Balances?>(null) }
    LaunchedEffect(version) {
        loaded = withContext(Dispatchers.IO) { Wallet.balances(context) }
    }
    // Until the first figure lands the balance is a wait, said as one; the
    // tiles and cards below need no wallet arithmetic and draw at once. This
    // used to return here, which left the whole home screen empty for as
    // long as the first decrypt of a large wallet took — several seconds
    // of a blank page on every cold start.
    val b = loaded
    if (b == null) {
        Box(
            Modifier.fillMaxWidth().padding(vertical = 48.dp),
            contentAlignment = Alignment.Center,
        ) {
            org.ducatproject.ducat.ui.CatSpinner(
                Modifier.size(40.dp), tint = MaterialTheme.colorScheme.primary,
            )
        }
    } else {
        // The capacity comes from `core::float` across the bridge, so the one
        // number §17.2 forbids overstating is computed by the same code the
        // conformance vectors and the harness run, rather than a second
        // implementation in Kotlin.
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

    // The three jobs that belong on the personal screen, as one row of
    // squares. Hailing is a rider's moment rather than an operating mode, and
    // looking for a car or a place is the same shape of moment — things a
    // person does occasionally, not jobs they run. The modes are for whoever
    // *has* a car or a room to let (§16.18).
    val hailSheet = remember { mutableStateOf(false) }
    Spacer(Modifier.height(12.dp))
    org.ducatproject.ducat.ui.HomeTiles(
        onHail = { hailSheet.value = true },
        // Every one of these hands the app to the mode whose job it is, rather
        // than opening a sheet over the wallet. Browsing, listing and the
        // deals that follow are one job somebody settles into; a sheet is a
        // thing held open on top of something else.
        //
        // The three renting nouns share a mode and say which door they came
        // through, which is the whole difference between them.
        onBrowse = { k ->
            val modes = org.ducatproject.ducat.ModeStore(context)
            // browsing = true: this is somebody on their own home screen
            // going to look at what is nearby, not somebody starting a shift.
            // The shell grows a way back for it — see ModeStore.set.
            when (k) {
                org.ducatproject.ducat.Listings.KIND_SALE ->
                    modes.set(org.ducatproject.ducat.Mode.Marketplace, browsing = true)
                org.ducatproject.ducat.Listings.KIND_SKILL ->
                    modes.set(org.ducatproject.ducat.Mode.HireHelp, browsing = true)
                else -> {
                    MainActivity.browseKind.value = k
                    modes.set(org.ducatproject.ducat.Mode.Renting, browsing = true)
                }
            }
        },
    )
    // Both flows live on, tiles or not: the hail keeps its card for a hail
    // that is actually standing, and each owns its own screens.
    org.ducatproject.ducat.ui.HailCard(sheetState = hailSheet)

    // Whose turn it is, on the screen somebody opens without being asked to.
    //
    // A settlement proposal reaches the other phone as one notification. If it
    // is swiped away, arrives face-down, or lands while notifications are off,
    // the app afterwards looks exactly as it does when nothing is happening —
    // a balance and six tiles — while on the other side somebody's money sits
    // in an escrow that cannot pay out without a second signature, and they
    // conclude they are being ignored. The same is true of a deal waiting on
    // money this device owes.
    val waiting = remember(version) { org.ducatproject.ducat.Ceremony.waitingOnMe(context) }
    if (waiting.isNotEmpty()) {
        val contacts = remember(version) { ContactStore(context).all() }
        Spacer(Modifier.height(12.dp))
        waiting.take(3).forEach { o ->
            val peerHex = org.ducatproject.ducat.Ceremony.otherPrincipal(o)
            val peer = contacts.firstOrNull { it.personaHex == peerHex }
            val who = peer?.displayName()
                ?: androidx.compose.ui.res.stringResource(R.string.shells_booking_someone)
            Surface(
                color = MaterialTheme.colorScheme.secondaryContainer,
                shape = MaterialTheme.shapes.large,
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
            ) {
                Row(
                    Modifier
                        .clickable { peer?.let { onOpenChat(it) } }
                        .padding(14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text(
                            androidx.compose.ui.res.stringResource(
                                if (o.optString("stage") == "release_pending") {
                                    R.string.main_waiting_sign
                                } else {
                                    R.string.main_waiting_pay
                                },
                                who,
                            ),
                            style = MaterialTheme.typography.titleSmall,
                        )
                        Text(
                            androidx.compose.ui.res.stringResource(R.string.main_waiting_body),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    Spacer(Modifier.width(8.dp))
                    Icon(
                        Icons.AutoMirrored.Filled.KeyboardArrowRight,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }

    // The way back to what you are offering.
    //
    // Listing is reachable from the search screens now, which is where the
    // thought occurs — but the things listed are only *managed* in the
    // Renting mode, three taps down a drawer. Somebody who sold a bicycle
    // from the Marketplace screen would have had nowhere to go to take it
    // down again, which is half a feature. Tapping this hands the app to that
    // mode — as a look, not a shift: this row is on the wallet's own home
    // screen, and it used to enter the mode the way the drawer does, which
    // put the bound persona's hat on, gave Back nothing to return to (it
    // left the app, and relaunching came back into Marketplace), and opened
    // on Browse when the row had promised the listings.
    // One row per mode that holds any, because they are managed in two places
    // now: a bicycle for sale in Marketplace, a room in Renting. A single row
    // would have to guess which, and would be wrong for anybody who offers
    // both — the cost of the split, paid here rather than by the person
    // hunting for a listing that is not where the row sent them.
    val mine = remember(version) { org.ducatproject.ducat.Listings.all(context) }
    val groups = remember(mine) {
        fun n(kinds: List<Int>) = mine.count { it.optInt("kind") in kinds }
        listOf(
            org.ducatproject.ducat.Mode.Marketplace to
                n(org.ducatproject.ducat.Listings.SALE_KINDS),
            org.ducatproject.ducat.Mode.Renting to
                n(org.ducatproject.ducat.Listings.RENT_KINDS),
            org.ducatproject.ducat.Mode.HireHelp to
                n(org.ducatproject.ducat.Listings.SKILL_KINDS),
        ).filter { it.second > 0 }
    }
    groups.forEach { (m, n) ->
        Spacer(Modifier.height(12.dp))
        Surface(
            color = MaterialTheme.colorScheme.surfaceVariant,
            shape = MaterialTheme.shapes.large,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        ) {
            Row(
                Modifier
                    .clickable {
                        org.ducatproject.ducat.ModeStore(context).set(m, browsing = true)
                        // What you are offering sits second in all three
                        // shells; the shell honours this after it composes.
                        org.ducatproject.ducat.ui.shellTabRequest.value = 1
                    }
                    .padding(14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(
                        androidx.compose.ui.res.stringResource(
                            when (m) {
                                org.ducatproject.ducat.Mode.Marketplace ->
                                    R.string.mode_marketplace
                                org.ducatproject.ducat.Mode.HireHelp ->
                                    R.string.mode_hire_help
                                else -> R.string.mode_renting
                            },
                        ),
                        style = MaterialTheme.typography.titleSmall,
                    )
                    Text(
                        androidx.compose.ui.res.pluralStringResource(
                            R.plurals.main_listings_count, n, n,
                        ),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Spacer(Modifier.width(8.dp))
                Icon(
                    Icons.AutoMirrored.Filled.KeyboardArrowRight,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }

    // §16.20 on the front porch: a subscription whose issues arrive while
    // the phone is pocketed must not depend on someone remembering a
    // drawer. One card, only when something is actually waiting.
    val waitingIssues = remember(version) {
        org.ducatproject.ducat.Publications.subscribedPublishers(context)
            .filterNot { org.ducatproject.ducat.Publications.isMuted(context, it) }
            .sumOf { pub ->
                val sub = org.ducatproject.ducat.Publications.subscription(context, pub)
                sub?.third?.keys?.count { period ->
                    org.ducatproject.ducat.ui.LibraryFetch
                        .fetchedBytes(context, pub, period) == null
                } ?: 0
            }
    }
    if (waitingIssues > 0) {
        Spacer(Modifier.height(12.dp))
        Surface(
            color = MaterialTheme.colorScheme.surfaceVariant,
            shape = MaterialTheme.shapes.large,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        ) {
            Row(
                Modifier.clickable { onLibrary() }.padding(14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(
                        androidx.compose.ui.res.stringResource(R.string.section_library),
                        style = MaterialTheme.typography.titleSmall,
                    )
                    Text(
                        androidx.compose.ui.res.pluralStringResource(
                            R.plurals.main_issues_waiting, waitingIssues, waitingIssues,
                        ),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Spacer(Modifier.width(8.dp))
                Icon(
                    Icons.AutoMirrored.Filled.KeyboardArrowRight,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }


    // The deal that cannot move without you, said where you already look.
    // waitingOnMe is precise about whose turn it is — "waiting to be
    // funded" is true of both sides and useful to neither — and the row
    // routes to the Deals tab of the mode whose job the deal was.
    val waitingDeals = remember(version) {
        runCatching {
            org.ducatproject.ducat.Ceremony.waitingOnMe(context).filter {
                it.optInt("kind") == org.ducatproject.ducat.Ceremony.KIND_RESERVATION
            }
        }.getOrDefault(emptyList())
    }
    if (waitingDeals.isNotEmpty()) {
        Spacer(Modifier.height(12.dp))
        Surface(
            color = MaterialTheme.colorScheme.surfaceVariant,
            shape = MaterialTheme.shapes.large,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        ) {
            Row(
                Modifier.clickable {
                    val about = waitingDeals.first().optInt("aboutKind", 0)
                    val mode = when (about) {
                        org.ducatproject.ducat.Listings.KIND_SALE ->
                            org.ducatproject.ducat.Mode.Marketplace
                        org.ducatproject.ducat.Listings.KIND_SKILL ->
                            org.ducatproject.ducat.Mode.HireHelp
                        else -> org.ducatproject.ducat.Mode.Renting
                    }
                    org.ducatproject.ducat.ModeStore(context).set(mode, browsing = true)
                    // Their Deals tabs all sit third; the shell honours this
                    // after it composes.
                    org.ducatproject.ducat.ui.shellTabRequest.value = 2
                }.padding(14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(
                        androidx.compose.ui.res.stringResource(R.string.main_deals_title),
                        style = MaterialTheme.typography.titleSmall,
                    )
                    Text(
                        androidx.compose.ui.res.pluralStringResource(
                            R.plurals.main_deals_waiting,
                            waitingDeals.size, waitingDeals.size,
                        ),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Spacer(Modifier.width(8.dp))
                Icon(
                    Icons.AutoMirrored.Filled.KeyboardArrowRight,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }

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
                    Icons.AutoMirrored.Filled.KeyboardArrowRight,
                    contentDescription = androidx.compose.ui.res
                        .stringResource(R.string.main_open_backup_settings),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }

    // The other half of the runtime ask above: a decline is silent, and
    // after it the app is muted — no bill, no ring, no receipt — while
    // looking exactly as it does when nothing is happening. Said here, once,
    // as a card with the way back; re-read on every resume so it leaves the
    // moment they turn them back on in Settings. Notify.muted, not the app
    // switch alone: a channel turned off on the same Settings page mutes
    // the same bills and calls, and the card was blind to it.
    val lifecycle = androidx.compose.ui.platform.LocalLifecycleOwner.current
    var notifyOff by remember { mutableStateOf(Notify.muted(context)) }
    DisposableEffect(lifecycle) {
        val watch = androidx.lifecycle.LifecycleEventObserver { _, event ->
            if (event == androidx.lifecycle.Lifecycle.Event.ON_RESUME) {
                notifyOff = Notify.muted(context)
            }
        }
        lifecycle.lifecycle.addObserver(watch)
        onDispose { lifecycle.lifecycle.removeObserver(watch) }
    }
    if (notifyOff) {
        Spacer(Modifier.height(12.dp))
        Surface(
            color = MaterialTheme.colorScheme.surfaceVariant,
            shape = MaterialTheme.shapes.large,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        ) {
            Row(
                Modifier
                    .clickable {
                        runCatching {
                            context.startActivity(
                                android.content.Intent(
                                    android.provider.Settings.ACTION_APP_NOTIFICATION_SETTINGS,
                                ).putExtra(
                                    android.provider.Settings.EXTRA_APP_PACKAGE,
                                    context.packageName,
                                ),
                            )
                        }
                    }
                    .padding(14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(
                        androidx.compose.ui.res.stringResource(R.string.main_notify_off_title),
                        style = MaterialTheme.typography.titleSmall,
                    )
                    Text(
                        androidx.compose.ui.res.stringResource(R.string.main_notify_off_body),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Spacer(Modifier.width(8.dp))
                Icon(
                    Icons.AutoMirrored.Filled.KeyboardArrowRight,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }

    // The last few movements, right under the number they explain — the shape
    // every payments app the user knows leads with. Three rows, then the tab.
    //
    // Built off the main thread: the ledger reads the wallet, the receipts
    // and an encrypted prefs table, and doing that inside composition froze
    // the whole app for as long as the ledger was long (an ANR at ~60 rows).
    // The rows appearing a beat after the balance is the correct trade.
    var recent by remember { mutableStateOf<List<Ledger.Event>>(emptyList()) }
    LaunchedEffect(version) {
        recent = withContext(Dispatchers.IO) { Ledger.build(context).take(3) }
    }
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
                        // The same sentence the Activity screen uses. This row
                        // used to fall back to the bare word "Sent", which the
                        // arrow beside it and the minus sign after it were
                        // already saying — while dropping the one thing they
                        // could not say, which is where the money went.
                        org.ducatproject.ducat.ui.who(context, e),
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
        // labelSmall, to match the centre slot rather than tower over it.
        // "Send/Receive" is the longest word in the bar and sits in its
        // narrowest fifth, so it sets the size and these four follow.
        label = {
            Text(
                androidx.compose.ui.res.stringResource(target.labelRes),
                style = MaterialTheme.typography.labelSmall,
                maxLines = 1,
                softWrap = false,
            )
        },
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
