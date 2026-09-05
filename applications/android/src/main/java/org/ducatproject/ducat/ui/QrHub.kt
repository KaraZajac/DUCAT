package org.ducatproject.ducat.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.delay
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.R

private const val TAG = "QrHub"

/**
 * One place for codes: read one, or show yours.
 *
 * There is no "show to pay" third tab, and the reason is structural rather than
 * an omission. A payment code has to name an amount, and an amount belongs to a
 * particular sale — so that screen is the till (Point of sale), which knows what
 * it is charging. A standing "pay me" code with no amount is just this card.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun QrHub(
    onOpenChat: (Contact) -> Unit,
    /** A Monero code is not a contact; it is a payment about to happen. */
    onScanAddress: (String, Long) -> Unit,
    onClose: () -> Unit,
) {
    val context = LocalContext.current
    var scanning by remember { mutableStateOf(true) }
    var uri by remember { mutableStateOf(ContactStore(context).currentCardUri()) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    // The scan tab's own: a claim on its way, and what the last code came
    // to. These used to share the pair above, which only My code rendered
    // — so a code that was neither card nor address, or a card whose claim
    // failed, left a live preview that said nothing, with the sentence on
    // the other tab.
    // The claim runs off the screen (claimOffScreen), and this is which
    // card it is for. Saveable, because a rotation mid-claim rebuilt this
    // screen with `claiming` false and a fresh camera: the same code was
    // read again and a second claim raced the first for the card's one
    // reply slot, and the thread the first one opened was never shown.
    var claimingCard by rememberSaveable { mutableStateOf<String?>(null) }
    var claiming by remember {
        mutableStateOf(claimingCard?.let { ThreadSends.inFlight(claimKey(it)) } ?: false)
    }
    var scanError by remember { mutableStateOf<String?>(null) }
    // Nothing to introduce ourselves with, and about to introduce ourselves.
    // See NameGate: the name travels on the handshake, so a blank one arrives
    // as "Unnamed contact" and neither end is told.
    var intro by remember { mutableStateOf<(() -> Unit)?>(null) }
    NameGate(
        open = intro != null,
        onDismiss = { intro = null },
        onNamed = { val go = intro; intro = null; go?.invoke() },
    )


    BackHandler(onBack = onClose)

    // **Look again when the registry changes.**
    //
    // This is the code somebody holds a phone out with, and it is claim-once.
    // The moment a scanner answers it, collectClaims adopts them and mints the
    // replacement in the same pass — but the URI here was read once when the
    // screen opened, so the QR on screen went on being the dead one. Hold the
    // phone out to a second person and they get "card already claimed", with
    // nothing on either screen to say why, while a perfectly good replacement
    // sat in the registry.
    //
    // currentCardUri already answers "the newest profile card nobody has
    // answered", so this only has to ask it again. Only when it has something:
    // a null means the claim landed and the replacement is a moment behind,
    // and blanking the screen for that moment would be its own little lie.
    val cardsV by ContactStore.changes.collectAsState()
    val personas = remember { PersonaStore(context) }
    val worn = remember(cardsV) { personas.worn() }

    // A mint asked for by hand, after one failed. Any value above zero is
    // "now, and skip the grace below".
    var attempt by remember { mutableIntStateOf(0) }

    // The mint is the process's (ThreadSends), keyed to the hat, and `busy`
    // is read from there rather than kept here: kept here, a rotation
    // mid-mint recreated this screen with busy false, and for a hat that
    // had never had a code the effect below met no guard at all and
    // minted a second card while the first was still being written.
    val mintKey = "mint:$worn"
    val tick by ThreadSends.ticks.collectAsState()
    LaunchedEffect(tick) {
        busy = ThreadSends.inFlight(mintKey)
        for (o in ThreadSends.take(mintKey)) when (o) {
            is ThreadSends.Outcome.Landed -> { o.result?.let { uri = it }; error = null }
            is ThreadSends.Outcome.Failed -> {
                // The link sentence is the mapper's default, and there
                // is no link here: nothing was scanned, a record was
                // not written.
                error = moneyFailure(context, o.error, fallback = R.string.qrhub_issue_failed)
                DucatLog.w(TAG, "issue: ${o.error.javaClass.simpleName}: ${o.error.message}")
            }
        }
    }
    // The scanned card's claim, read back by whichever instance of this
    // screen is up when it lands.
    LaunchedEffect(tick, claimingCard) {
        val k = claimingCard?.let(::claimKey) ?: return@LaunchedEffect
        for (o in ThreadSends.take(k)) {
            claimingCard = null
            when (o) {
                is ThreadSends.Outcome.Landed -> o.claimed(context)?.let(onOpenChat)
                is ThreadSends.Outcome.Failed -> {
                    scanError = context.getString(claimFailureRes(o.error))
                    DucatLog.w(TAG, "claim: ${o.error.message}")
                }
            }
        }
        claiming = claimingCard != null && ThreadSends.inFlight(k)
    }

    // One effect for both lives of the code. The registry answers scoped to
    // the worn persona now, so switching hats re-reads; a hat that has NEVER
    // had a standing code gets one minted here (the first-run case, per
    // persona) — but a code that just got *claimed* is left alone, because
    // collectClaims pre-issues the replacement and a second mint here would
    // put two cards up for one hat. "Never had" and "just claimed" are told
    // apart by whether any profile card for this hat exists at all.
    LaunchedEffect(cardsV, worn, attempt) {
        val store = ContactStore(context)
        val current = store.currentCardUri()
        if (current != null) {
            if (current != uri) uri = current
            return@LaunchedEffect
        }
        val primary = personas.personaHex()
        val everHad = store.issuedCards().any {
            it.purpose == "profile" &&
                (it.owner == worn || (it.owner.isBlank() && worn == primary))
        }
        if (ThreadSends.inFlight(mintKey)) return@LaunchedEffect
        if (everHad && attempt == 0) {
            // Left alone for a while, not for good. The pre-issue is one
            // DHT write behind the claim, and when it lands the store bumps
            // and this effect restarts with the code in hand — but when it
            // fails ("could not pre-issue", a line in the log and nothing
            // else) the answered card stays in the registry for an hour,
            // and for that hour this screen said "Publishing…" over a mint
            // nobody was doing. A replacement that has not arrived in this
            // long is not coming.
            delay(REPLACEMENT_GRACE_MS)
        }
        // After the grace, not before it: a mint that landed during the
        // wait restarted this effect with the code in hand.
        if (ThreadSends.inFlight(mintKey)) return@LaunchedEffect
        busy = true
        error = null
        // Its own job, not this effect's. issueCard bumps the store midway,
        // which restarts the effect — and a restart cancelled the mint's
        // continuation with `busy` still true, so My code spun for as long
        // as the screen was open; an unrelated bump landing in the same
        // seconds found no card and no history and minted a second one for
        // the same hat. The guard above is what the restart now meets.
        val hat = worn
        ThreadSends.launch(store, mintKey, null) {
            Mailbox.issueCard(
                context, MyProfile(context).name(), 60uL * 60uL * 24uL,
                asPersonaHex = hat,
            ).uri
        }
    }

    Dialog(
        onDismissRequest = onClose,
        properties = fullScreenDialogProperties(),
    ) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            Scaffold(
                topBar = {
                    TopAppBar(
                        colors = TopAppBarDefaults.topAppBarColors(
                        containerColor = MaterialTheme.colorScheme.background,
                    ),
                                            title = {},
                        navigationIcon = {
                            IconButton(onClick = onClose) {
                                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.qrhub_close))
                            }
                        },
                    )
                },
            ) { padding ->
                Column(Modifier.padding(padding).fillMaxSize()) {
                    // Above the content, centred, its own row. In the app bar's
                    // title slot it had to share the width with the back arrow
                    // and was squeezed to nothing.
                    // Full width and one line each. Sized to content it was
                    // narrow enough that both labels wrapped, so a two-word
                    // button came out as two rows of text.
                    SingleChoiceSegmentedButtonRow(
                        Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 16.dp, vertical = 4.dp),
                    ) {
                        SegmentedButton(
                            selected = scanning,
                            // A fresh tab is a fresh scanner: the camera is
                            // remounted with nothing latched, so the same
                            // card can be tried again — and the old verdict
                            // should not be waiting above it.
                            onClick = { scanning = true; scanError = null },
                            shape = ducatSegmentShape(0, 2),
                            // No checkmark — the fill already says which is active,
                            // and the icon shoves the label sideways when it appears.
                            icon = {},
                            modifier = Modifier.weight(1f),
                        ) { Text(stringResource(R.string.qrhub_scan_code), maxLines = 1, softWrap = false) }
                        SegmentedButton(
                            selected = !scanning,
                            onClick = { scanning = false },
                            shape = ducatSegmentShape(1, 2),
                            // No checkmark — the fill already says which is active,
                            // and the icon shoves the label sideways when it appears.
                            icon = {},
                            modifier = Modifier.weight(1f),
                        ) { Text(stringResource(R.string.qrhub_my_code), maxLines = 1, softWrap = false) }
                    }
                    if (scanning) {
                        // What the camera is doing about the last code, above
                        // the preview where the eye already is.
                        if (claiming) {
                            Row(
                                Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 4.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                                Spacer(Modifier.width(10.dp))
                                Text(
                                    stringResource(R.string.contacts_reading_inbox),
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                        }
                        scanError?.let {
                            Text(
                                it,
                                Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 4.dp),
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.error,
                            )
                        }
                        // One scanner for both kinds. A person holding a phone at
                        // a code does not know or care which sort it is, and two
                        // buttons for "scan" would make them guess.
                        // Content, not a dialog: this screen already is one,
                        // and a nested dialog painted over its own tab bar —
                        // which is why the toggle appeared not to exist.
                        QrScannerContent(
                            prompt = stringResource(R.string.qrhub_scan_prompt),
                            onResult = { raw ->
                                val text = raw.trim()
                                if (claiming) {
                                    // A second code while the first is being
                                    // claimed would be a second claim; the
                                    // scanner already swallows the same one.
                                } else if (!text.startsWith("ducat:card/")) {
                                    // A Monero code. Not a contact and never
                                    // becomes one — hand it to the pay screen,
                                    // which is what the person scanning it
                                    // wanted in the first place. The amount
                                    // goes with it: a code that named one was
                                    // being read for its address alone, and
                                    // the payer left to retype the figure.
                                    val m = moneroUri(text)
                                    if (m != null) onScanAddress(m.first, m.second)
                                    else scanError = context.getString(R.string.qrhub_not_a_code)
                                } else {
                                    val go: () -> Unit = {
                                        claiming = true; scanError = null
                                        claimingCard = text
                                        // Scanned this one before: the thread
                                        // it opened is the answer, and the
                                        // claim finds it.
                                        claimOffScreen(context, text)
                                    }
                                    if (nameGateNeeded(context)) intro = go else go()
                                }
                            },
                        )
                    } else {
                        // Which hat the code belongs to, said above it —
                        // this is the screen held out to a person, and the
                        // one place wearing the wrong hat would bind a
                        // stranger to the wrong compartment.
                        val roster = remember(cardsV) { personas.all() }
                        if (roster.size > 1) {
                            val wornP = roster.firstOrNull { it.hex == worn }
                            if (wornP != null) {
                                Row(
                                    Modifier.fillMaxWidth().padding(bottom = 6.dp),
                                    horizontalArrangement = Arrangement.Center,
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    PersonaDot(wornP)
                                    Spacer(Modifier.width(6.dp))
                                    Text(
                                        stringResource(
                                            R.string.qrhub_showing_as, personaLabel(wornP),
                                        ),
                                        style = MaterialTheme.typography.labelMedium,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            }
                        }
                        MyCode(
                            uri = uri,
                            busy = busy,
                            error = error,
                            onCopy = {
                                uri?.let { copyText(context, it, context.getString(R.string.qrhub_copied)) }
                            },
                            onRetry = { error = null; attempt++ },
                        )
                    }
                }
            }
        }
    }
}

/** How long the claim's own replacement gets before this screen mints one. */
private const val REPLACEMENT_GRACE_MS = 30_000L

@Composable
private fun MyCode(
    uri: String?,
    busy: Boolean,
    error: String?,
    onCopy: () -> Unit,
    onRetry: () -> Unit,
) {
    val context = LocalContext.current
    // While this screen shows the code, a tap serves the same card. The QR
    // and the antenna are one offer in two physics.
    DisposableEffect(uri) {
        org.ducatproject.ducat.nfc.Tap.offered = uri
        onDispose { org.ducatproject.ducat.nfc.Tap.offered = null }
    }
    // Keyed on the store, because this is the screen you hold up to
    // somebody. A name or a picture set a minute ago and not shown here is
    // wrong in front of a person, which is the worst place for it.
    val profileV by ContactStore.changes.collectAsState()
    val name = remember(profileV) { MyProfile(context).name() }
    val pic = remember(profileV) { MyProfile(context).avatar() }
    // **A code on a phone that has not joined is a code nobody can take.**
    //
    // The two records behind it are published to the network, and until
    // this node is on it there is nothing out there to open: the person it
    // is handed to gets "not readable yet" and no idea whose end the
    // trouble is at. Caught on a phone whose node never attached — it held
    // up a perfectly ordinary-looking QR for as long as anyone cared to
    // scan it. Optimistic default, so a screen that is about to say
    // "connected" does not flash a warning on the way there.
    var joined by remember { mutableStateOf(true) }
    LaunchedEffect(Unit) {
        while (true) {
            joined = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                runCatching { uniffi.ducat_mobile.nodeStatus().publicInternetReady }
                    .getOrDefault(false)
            }
            delay(3_000)
        }
    }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Avatar(name ?: "?", pic, size = 72)
        Spacer(Modifier.height(10.dp))
        Text(name ?: stringResource(R.string.qrhub_no_name_set), style = MaterialTheme.typography.titleLarge)
        Spacer(Modifier.height(20.dp))

        when {
            // The failure before the spinner: with no code and a mint that
            // did not happen, this said "Publishing two records…" over the
            // sentence explaining that nothing was being published, and
            // the only way to try again was to leave and come back.
            !busy && uri == null && error != null -> Column(
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    error,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
                Spacer(Modifier.height(12.dp))
                OutlinedButton(onClick = onRetry) {
                    Text(stringResource(R.string.qrhub_try_again))
                }
            }
            busy || uri == null -> Column(horizontalAlignment = Alignment.CenterHorizontally) {
                CatSpinner(Modifier.size(40.dp), tint = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.height(12.dp))
                Text(
                    stringResource(R.string.qrhub_publishing),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            else -> {
                QrBlock(uri)
                if (!joined) {
                    Spacer(Modifier.height(12.dp))
                    Text(
                        stringResource(R.string.qrhub_offline),
                        color = MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.bodySmall,
                        textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                    )
                }
                Spacer(Modifier.height(16.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    OutlinedButton(onClick = onCopy) {
                        Icon(Icons.Filled.ContentCopy, null, Modifier.size(18.dp))
                        Spacer(Modifier.width(8.dp))
                        Text(stringResource(R.string.qrhub_copy_link))
                    }
                    OutlinedButton(onClick = {
                        val i = android.content.Intent(android.content.Intent.ACTION_SEND).apply {
                            type = "text/plain"
                            putExtra(android.content.Intent.EXTRA_TEXT, uri)
                        }
                        context.startActivity(
                            android.content.Intent.createChooser(
                                i, context.getString(R.string.qrhub_share_chooser),
                            )
                        )
                    }) {
                        Icon(Icons.Filled.Share, null, Modifier.size(18.dp))
                        Spacer(Modifier.width(8.dp))
                        Text(stringResource(R.string.qrhub_share))
                    }
                }
                Spacer(Modifier.height(20.dp))
                Text(
                    stringResource(R.string.qrhub_tap_or_scan),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(6.dp))
                Text(
                    // Worth saying because it is surprising, and because someone
                    // who does not know it will hand the same code to two people
                    // and wonder why the second one never arrives.
                    stringResource(R.string.qrhub_one_per_code),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(8.dp))
                Text(
                    stringResource(R.string.qrhub_name_travels),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.outline,
                )
            }
        }
        // Under a code that is still showing — the one this screen opened
        // with, after its claim, when the mint of the next failed. With no
        // code the branch above already said it.
        if (uri != null || busy) error?.let {
            Spacer(Modifier.height(16.dp))
            Text(it, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall)
        }
    }
}
