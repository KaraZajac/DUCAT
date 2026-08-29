package org.ducatproject.ducat.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.MyProfile
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
    val scope = rememberCoroutineScope()
    val clipboard = LocalClipboardManager.current
    var scanning by remember { mutableStateOf(true) }
    var uri by remember { mutableStateOf(ContactStore(context).currentCardUri()) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
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
    LaunchedEffect(cardsV) {
        ContactStore(context).currentCardUri()
            ?.takeIf { it != uri }
            ?.let { uri = it }
    }

    // Made without being asked for. A card takes seconds to publish — two DHT
    // records — and the moment someone wants it is the moment they are holding
    // a phone out to somebody. Waiting until then puts the wait in front of the
    // person it is for.
    LaunchedEffect(Unit) {
        if (uri != null) return@LaunchedEffect
        busy = true
        val r = withContext(Dispatchers.IO) {
            runCatching {
                Mailbox.issueCard(context, MyProfile(context).name(), 60uL * 60uL * 24uL)
            }
        }
        busy = false
        r.onSuccess { uri = it.uri }
            .onFailure {
                error = moneyFailure(context, it)
                DucatLog.w(TAG, "issue: ${it.message}")
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
                                Icon(Icons.Filled.ArrowBack, contentDescription = stringResource(R.string.qrhub_close))
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
                            onClick = { scanning = true },
                            shape = SegmentedButtonDefaults.itemShape(0, 2),
                            // No checkmark — the fill already says which is active,
                            // and the icon shoves the label sideways when it appears.
                            icon = {},
                            modifier = Modifier.weight(1f),
                        ) { Text(stringResource(R.string.qrhub_scan_code), maxLines = 1, softWrap = false) }
                        SegmentedButton(
                            selected = !scanning,
                            onClick = { scanning = false },
                            shape = SegmentedButtonDefaults.itemShape(1, 2),
                            // No checkmark — the fill already says which is active,
                            // and the icon shoves the label sideways when it appears.
                            icon = {},
                            modifier = Modifier.weight(1f),
                        ) { Text(stringResource(R.string.qrhub_my_code), maxLines = 1, softWrap = false) }
                    }
                    if (scanning) {
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
                                if (!text.startsWith("ducat:card/")) {
                                    // A Monero code. Not a contact and never
                                    // becomes one — hand it to the pay screen,
                                    // which is what the person scanning it
                                    // wanted in the first place. The amount
                                    // goes with it: a code that named one was
                                    // being read for its address alone, and
                                    // the payer left to retype the figure.
                                    val m = moneroUri(text)
                                    if (m != null) onScanAddress(m.first, m.second)
                                    else error = context.getString(R.string.qrhub_not_a_code)
                                } else {
                                    val go: () -> Unit = {
                                    busy = true; error = null
                                    scope.launch {
                                        val r = withContext(Dispatchers.IO) {
                                            runCatching {
                                                val card = uniffi.ducat_mobile.readContactCard(text)
                                                Mailbox.claimCard(context, card, null)
                                            }
                                        }
                                        busy = false
                                        r.onSuccess(onOpenChat).onFailure {
                                            error = context.getString(claimFailureRes(it))
                                            DucatLog.w(TAG, "claim: ${it.message}")
                                        }
                                    }
                                    }
                                    if (nameGateNeeded(context)) intro = go else go()
                                }
                            },
                        )
                    } else {
                        MyCode(
                            uri = uri,
                            busy = busy,
                            error = error,
                            onCopy = {
                                uri?.let { copyText(context, it, context.getString(R.string.qrhub_copied)) }
                            },
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun MyCode(uri: String?, busy: Boolean, error: String?, onCopy: () -> Unit) {
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

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Avatar(name ?: "?", pic, size = 72)
        Spacer(Modifier.height(10.dp))
        Text(name ?: stringResource(R.string.qrhub_no_name_set), style = MaterialTheme.typography.titleLarge)
        Spacer(Modifier.height(20.dp))

        when {
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
        error?.let {
            Spacer(Modifier.height(16.dp))
            Text(it, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall)
        }
    }
}
