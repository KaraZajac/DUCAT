package org.ducatproject.ducat.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.ArrowDownward
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.filled.RequestQuote
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.LockOpen
import androidx.compose.material.icons.filled.Send
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.WalletStore
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.threadAad
import org.ducatproject.ducat.StoredMessage
import org.ducatproject.ducat.Amounts
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import androidx.compose.material.icons.filled.Receipt
import androidx.compose.material.icons.filled.DirectionsCar
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import org.ducatproject.ducat.formatXmr
import org.ducatproject.ducat.DucatLog
import androidx.compose.material.icons.filled.Image
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.PhotoCamera
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.InsertDriveFile
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.LocationOn
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Stop
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.graphics.Color

/**
 * One conversation.
 *
 * Every outgoing message is sealed to one of their published prekeys (§16.11)
 * and chained to the one before it (§16.10). Neither is decoration: the chain
 * is what makes a *removed and replaced* message visible, and the prekey is
 * what makes a delivered message unrecoverable afterwards.
 *
 * The screen shows when a message went out **without** forward secrecy, because
 * §16.11 requires the fallback be visible rather than silently accepted.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatScreen(contact: Contact, onBack: () -> Unit) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val store = remember { ContactStore(context) }
    val scope = rememberCoroutineScope()
    var c by remember { mutableStateOf(contact) }
    val mine = remember { PersonaStore(context).personaHex() }
    var messages by remember { mutableStateOf(store.thread(contact.personaHex)) }
    // Reactions decorate their targets rather than being bubbles (§16.14):
    // the message is the unit of rendering, the reaction is a remark upon one.
    // Latest per (sender, target) wins, which is how changing your mind works.
    val reactions = remember(messages) {
        messages.filter { it.kind == 4 && it.reSeq != null }
            .associateBy { r -> Triple(r.outgoing == r.reOwn, r.reSeq!!, r.outgoing) }
    }
    var draft by remember { mutableStateOf("") }
    var sending by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val listState = rememberLazyListState()

    // §16.16: viewing the thread is what "read" means, so the watermark is
    // published from exactly here — never from the poller, which reads
    // everything and knows nothing about eyes.
    LaunchedEffect(c.inSeq) {
        if (store.readReceipts()) {
            kotlinx.coroutines.withContext(Dispatchers.IO) {
                runCatching { Mailbox.markRead(context, c) }
            }
        }
    }

    // Re-read whenever anything writes to the store. The responder runs in a
    // different coroutine and cannot reach this screen's state directly; without
    // this, an inbound message was decrypted and stored and then stayed
    // invisible until the user sent something of their own.
    val version by ContactStore.changes.collectAsState()
    LaunchedEffect(version) {
        messages = store.thread(c.personaHex)
        store.all().firstOrNull { it.personaHex == c.personaHex }?.let { c = it }
        // Looking at the thread is what "seen" means; the dot and the badge
        // clear the moment the eyes arrive, not when a reply goes out.
        store.setChatSeen(c.personaHex, c.inSeq)
    }

    LaunchedEffect(messages.size) {
        if (messages.isNotEmpty()) listState.animateScrollToItem(messages.size)
    }
    // The keyboard opening does not change the message count, so the scroll
    // above never fired for it and a sent message ended up behind the keyboard
    // until the user dismissed it. Watching the IME inset is what actually
    // tracks "the visible area just shrank".
    val imeBottom = WindowInsets.ime.getBottom(LocalDensity.current)
    LaunchedEffect(imeBottom) {
        if (messages.isNotEmpty()) listState.animateScrollToItem(messages.size)
    }

    var settingsOpen by remember { mutableStateOf(false) }
    var askOpen by remember { mutableStateOf(false) }
    var payRequest by remember { mutableStateOf<StoredMessage?>(null) }
    var billView by remember { mutableStateOf<StoredMessage?>(null) }
    var reserveOpen by remember { mutableStateOf(false) }
    var confirmDelete by remember { mutableStateOf<StoredMessage?>(null) }

    // Applied on open and whenever the thread changes, because nothing else
    // runs while a conversation sits idle.
    LaunchedEffect(version) {
        val secs = store.disappearAfter(c.personaHex)
        if (secs > 0 && store.expireOld(c.personaHex, secs) > 0) {
            messages = store.thread(c.personaHex)
        }
    }

    Scaffold(
        modifier = Modifier.imePadding(),
        topBar = {
            TopAppBar(
                colors = TopAppBarDefaults.topAppBarColors(
                containerColor = MaterialTheme.colorScheme.background,
            ),
                            title = { Text(c.displayName()) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.Filled.ArrowBack, stringResource(R.string.chat_back))
                    }
                },
                actions = {
                    IconButton(onClick = { settingsOpen = true }) {
                        Icon(Icons.Filled.MoreVert, stringResource(R.string.chat_conversation_settings))
                    }
                },
            )
        },
        bottomBar = {
            Surface(color = MaterialTheme.colorScheme.background) {
                Column {
                    error?.let {
                        Text(
                            it,
                            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp),
                            color = MaterialTheme.colorScheme.error,
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                    val doSend = doSend@{
                        val body = draft.trim()
                        if (body.isEmpty() || sending || c.theirBundle == null) return@doSend
                        sending = true
                        error = null
                        scope.launch {
                            val result = withContext(Dispatchers.IO) {
                                runCatching { sendOne(context, c, body, mine) }
                            }
                            sending = false
                            result.onSuccess { updated ->
                                c = updated
                                draft = ""
                                messages = store.thread(c.personaHex)
                            }.onFailure {
                                error = it.message
                                    ?: context.getString(R.string.chat_could_not_send)
                            }
                        }
                    }

                    // One door for every attachment-ish action: the + opens a
                    // tray panel, picking collapses it, and the next feature
                    // costs a tray slot instead of composer width. Signal's
                    // grammar on purpose: camera and mic live inside the field
                    // while it is empty, typing swaps the + for send.
                    var trayOpen by remember { mutableStateOf(false) }
                    var contactPick by remember { mutableStateOf(false) }
                    var recording by remember { mutableStateOf(false) }
                    var recSecs by remember { mutableStateOf(0) }
                    val recorder = remember { VoiceRecorder(context) }
                    LaunchedEffect(recording) {
                        recSecs = 0
                        while (recording) { kotlinx.coroutines.delay(1000); recSecs++ }
                    }

                    val afterSend: (Result<*>, String) -> Unit = { r, what ->
                        r.onSuccess { messages = store.thread(c.personaHex) }
                            .onFailure {
                                error = it.message
                                    ?: context.getString(R.string.chat_could_not_send_the, what)
                                DucatLog.w("Chat", "$what: ${it.message}")
                            }
                        sending = false
                    }
                    // A picture (§16.15): resized, sealed under a fresh key,
                    // parked in its own record, referenced from the message.
                    // The record on the network is noise to everyone but this
                    // thread.
                    val pickImage = androidx.activity.compose.rememberLauncherForActivityResult(
                        androidx.activity.result.contract.ActivityResultContracts.GetContent()
                    ) { uri ->
                        if (uri != null) {
                            sending = true
                            scope.launch(Dispatchers.IO) {
                                afterSend(runCatching { sendPicture(context, c, mine, uri) },
                                    context.getString(R.string.chat_what_picture))
                            }
                        }
                    }
                    val pickFile = androidx.activity.compose.rememberLauncherForActivityResult(
                        androidx.activity.result.contract.ActivityResultContracts.GetContent()
                    ) { uri ->
                        if (uri != null) {
                            sending = true
                            scope.launch(Dispatchers.IO) {
                                afterSend(runCatching { sendFile(context, c, mine, uri) },
                                    context.getString(R.string.chat_what_file))
                            }
                        }
                    }
                    // The camera hands its full frame to the same resize-seal
                    // path a gallery pick takes; the staging file lives in
                    // cache and is overwritten by the next shot. Saveable (as a
                    // string — Uri is not) because the camera app routinely
                    // kills this process while it has the screen: a plain
                    // remember came back null, and the shot it named was
                    // silently dropped.
                    var cameraUri by rememberSaveable {
                        mutableStateOf<String?>(null)
                    }
                    val takePhoto = androidx.activity.compose.rememberLauncherForActivityResult(
                        androidx.activity.result.contract.ActivityResultContracts.TakePicture()
                    ) { ok ->
                        val uri = cameraUri?.let(android.net.Uri::parse)
                        if (ok && uri != null) {
                            sending = true
                            scope.launch(Dispatchers.IO) {
                                afterSend(runCatching { sendPicture(context, c, mine, uri) },
                                    context.getString(R.string.chat_what_picture))
                            }
                        }
                    }
                    val launchCamera = {
                        val dir = java.io.File(context.cacheDir, "camera").apply { mkdirs() }
                        val uri = androidx.core.content.FileProvider.getUriForFile(
                            context, context.packageName + ".backups",
                            java.io.File(dir, "shot.jpg"),
                        )
                        cameraUri = uri.toString()
                        takePhoto.launch(uri)
                    }
                    // Declaring CAMERA in the manifest (the QR scanner needs
                    // it) means even the delegate-to-camera-app intent requires
                    // the grant — an Android quirk, not a choice.
                    val camPerm = androidx.activity.compose.rememberLauncherForActivityResult(
                        androidx.activity.result.contract.ActivityResultContracts.RequestPermission()
                    ) { granted -> if (granted) launchCamera() }
                    val micPerm = androidx.activity.compose.rememberLauncherForActivityResult(
                        androidx.activity.result.contract.ActivityResultContracts.RequestPermission()
                    ) { }
                    val granted = { p: String ->
                        context.checkSelfPermission(p) ==
                            android.content.pm.PackageManager.PERMISSION_GRANTED
                    }
                    val sendLocation = {
                        trayOpen = false
                        grabLocation(context) { place ->
                            if (place == null) {
                                error = context.getString(R.string.chat_error_location_fix)
                            } else {
                                scope.launch(Dispatchers.IO) {
                                    afterSend(
                                        runCatching { Mailbox.send(context, c, place, mine) },
                                        context.getString(R.string.chat_what_location),
                                    )
                                }
                            }
                        }
                    }
                    val locPerm = androidx.activity.compose.rememberLauncherForActivityResult(
                        androidx.activity.result.contract.ActivityResultContracts.RequestPermission()
                    ) { ok -> if (ok) sendLocation() }

                    Row(
                        Modifier.padding(12.dp).fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        OutlinedTextField(
                            value = draft,
                            onValueChange = { if (it.length <= 2000) draft = it },
                            placeholder = {
                                Text(
                                    if (recording) {
                                        stringResource(R.string.chat_recording_placeholder, recSecs)
                                    } else stringResource(R.string.chat_message_placeholder),
                                    color = if (recording) MaterialTheme.colorScheme.error
                                    else Color.Unspecified,
                                )
                            },
                            keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                                imeAction = androidx.compose.ui.text.input.ImeAction.Send,
                            ),
                            keyboardActions = androidx.compose.foundation.text.KeyboardActions(
                                onSend = { doSend() },
                            ),
                            trailingIcon = {
                                if (draft.isBlank()) Row(
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    IconButton(
                                        onClick = {
                                            if (granted(android.Manifest.permission.CAMERA)) {
                                                launchCamera()
                                            } else {
                                                camPerm.launch(android.Manifest.permission.CAMERA)
                                            }
                                        },
                                        enabled = c.theirBundle != null && !sending,
                                    ) {
                                        Icon(
                                            Icons.Filled.PhotoCamera,
                                            stringResource(R.string.chat_take_picture),
                                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                    }
                                    // Hold to record, release to send — the
                                    // Signal gesture. Too short to be speech is
                                    // discarded rather than sent, so a stray
                                    // brush of the icon costs nothing.
                                    Box(
                                        Modifier
                                            .size(44.dp)
                                            .pointerInput(c.theirBundle != null) {
                                                if (c.theirBundle == null) return@pointerInput
                                                detectTapGestures(onPress = {
                                                    if (!granted(android.Manifest.permission.RECORD_AUDIO)) {
                                                        micPerm.launch(
                                                            android.Manifest.permission.RECORD_AUDIO
                                                        )
                                                        return@detectTapGestures
                                                    }
                                                    if (!recorder.start()) return@detectTapGestures
                                                    recording = true
                                                    tryAwaitRelease()
                                                    recording = false
                                                    val memo = recorder.stop()
                                                    if (memo != null) {
                                                        sending = true
                                                        scope.launch(Dispatchers.IO) {
                                                            afterSend(
                                                                runCatching {
                                                                    sendVoice(context, c, mine, memo)
                                                                },
                                                                context.getString(
                                                                    R.string.chat_what_voice_memo
                                                                ),
                                                            )
                                                        }
                                                    }
                                                })
                                            },
                                        contentAlignment = Alignment.Center,
                                    ) {
                                        Icon(
                                            Icons.Filled.Mic,
                                            stringResource(R.string.chat_hold_to_record),
                                            tint = if (recording) MaterialTheme.colorScheme.error
                                            else MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                    }
                                } else {
                                    IconButton(onClick = { trayOpen = !trayOpen }) {
                                        Icon(
                                            Icons.Filled.Add, stringResource(R.string.chat_attach),
                                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                    }
                                }
                            },
                            modifier = Modifier.weight(1f),
                            maxLines = 4,
                        )
                        Spacer(Modifier.width(8.dp))
                        if (draft.isBlank()) {
                            FilledIconButton(
                                onClick = { trayOpen = !trayOpen },
                                enabled = c.theirBundle != null,
                            ) {
                                Icon(
                                    if (trayOpen) Icons.Filled.Close else Icons.Filled.Add,
                                    if (trayOpen) stringResource(R.string.chat_close)
                                    else stringResource(R.string.chat_attach),
                                )
                            }
                        } else {
                            FilledIconButton(
                                onClick = doSend,
                                enabled = !sending && c.theirBundle != null,
                            ) {
                                if (sending) {
                                    CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                                } else {
                                    Icon(Icons.Filled.Send, stringResource(R.string.chat_send))
                                }
                            }
                        }
                    }

                    androidx.compose.animation.AnimatedVisibility(visible = trayOpen) {
                        Row(
                            Modifier.fillMaxWidth().padding(bottom = 12.dp),
                            horizontalArrangement = Arrangement.SpaceEvenly,
                        ) {
                            TrayItem(
                                Icons.Filled.Image, stringResource(R.string.chat_gallery),
                                enabled = !sending,
                            ) {
                                trayOpen = false; pickImage.launch("image/*")
                            }
                            TrayItem(
                                Icons.Filled.InsertDriveFile, stringResource(R.string.chat_file),
                                enabled = !sending,
                            ) {
                                trayOpen = false; pickFile.launch("*/*")
                            }
                            // The cat: the app's money button everywhere, drawn
                            // as an Image because tinting it makes it a blob.
                            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                                FilledTonalIconButton(
                                    onClick = { trayOpen = false; askOpen = true },
                                    modifier = Modifier.size(52.dp),
                                ) {
                                    androidx.compose.foundation.Image(
                                        androidx.compose.ui.res.painterResource(
                                            org.ducatproject.ducat.R.drawable.ducat_cat
                                        ),
                                        contentDescription =
                                            stringResource(R.string.chat_money_desc),
                                        modifier = Modifier.size(30.dp),
                                    )
                                }
                                Text(
                                    stringResource(R.string.chat_money),
                                    style = MaterialTheme.typography.labelSmall,
                                )
                            }
                            TrayItem(
                                Icons.Filled.Person, stringResource(R.string.chat_contact),
                                enabled = !sending,
                            ) {
                                trayOpen = false; contactPick = true
                            }
                            TrayItem(
                                Icons.Filled.Lock, stringResource(R.string.res_tray),
                                enabled = !sending,
                            ) {
                                trayOpen = false; reserveOpen = true
                            }
                            TrayItem(
                                Icons.Filled.LocationOn, stringResource(R.string.chat_location),
                                enabled = !sending,
                            ) {
                                if (granted(android.Manifest.permission.ACCESS_FINE_LOCATION)) {
                                    sendLocation()
                                } else {
                                    locPerm.launch(
                                        android.Manifest.permission.ACCESS_FINE_LOCATION
                                    )
                                }
                            }
                        }
                    }

                    if (contactPick) {
                        ContactPickDialog(
                            contacts = store.all().filter { it.personaHex != c.personaHex }
                                .sortedBy { it.displayName().lowercase() },
                            // The introduction, done the only way consent
                            // allows: a fresh card of *mine*, dropped into the
                            // thread as a ducat: link, for them to hand to
                            // whoever should reach me. Their claim arrives
                            // through the same registry as any other card.
                            onIntroduceMe = {
                                contactPick = false
                                sending = true
                                scope.launch(Dispatchers.IO) {
                                    afterSend(
                                        runCatching {
                                            val card = Mailbox.issueCard(
                                                context,
                                                org.ducatproject.ducat.MyProfile(context).name(),
                                                60uL * 60uL * 24uL * 7uL,
                                                purpose = "intro",
                                            )
                                            Mailbox.send(
                                                context, c,
                                                context.getString(
                                                    R.string.chat_intro_card_body, card.uri
                                                ),
                                                mine,
                                            )
                                        },
                                        context.getString(R.string.chat_what_card),
                                    )
                                }
                            },
                            onPick = { chosen ->
                                contactPick = false
                                scope.launch(Dispatchers.IO) {
                                    afterSend(
                                        runCatching {
                                            Mailbox.send(context, c, contactCard(chosen), mine)
                                        },
                                        context.getString(R.string.chat_what_contact),
                                    )
                                }
                            },
                            onDismiss = { contactPick = false },
                        )
                    }
                    if (c.theirBundle == null) {
                        Text(
                            stringResource(R.string.chat_no_keys),
                            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
        // §15.12: the ride's escrow, carried where both parties already are.
        // One banner serves rider and driver — the roles come from the
        // ceremony's own frame, and each stage shows its one next action.
        RideBondBanner(c)
        LazyColumn(
            Modifier.weight(1f).fillMaxWidth(),
            state = listState,
            contentPadding = PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            item {
                Text(
                    stringResource(R.string.chat_e2e_notice),
                    Modifier.fillMaxWidth().padding(bottom = 12.dp),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                )
            }
            // 4 is a read receipt. 8 and 9 are ceremony rounds — a DKG share
            // and a FROST signing round, carried as messages so the thread
            // stays a complete record, but they are machinery: they have no
            // amount and nothing a person is meant to do about them. Rendered
            // as bubbles they came out as "You sent 0.000000 XMR — bond: your
            // share", which reads like a failed payment. The banner above
            // narrates the same ceremony in words, with a spinner.
            items(messages.filter { it.kind != 4 && it.kind != 8 && it.kind != 9 }) { m ->
                if (m.kind == 5) {
                    // A retraction is a remark about the thread, not a message
                    // in it: one quiet centred line, no bubble and no buttons —
                    // the bill or offer it names greys out where it stands.
                    val retractLine = if (m.reOwn) {
                        if (m.outgoing) stringResource(R.string.chat_you_withdrew)
                        else stringResource(R.string.chat_they_withdrew, c.displayName())
                    } else {
                        if (m.outgoing) stringResource(R.string.chat_you_declined)
                        else stringResource(R.string.chat_they_declined, c.displayName())
                    }
                    Text(
                        if (m.body.isNotBlank()) {
                            stringResource(R.string.chat_retract_with_quote, retractLine, m.body)
                        } else retractLine,
                        Modifier.fillMaxWidth().padding(vertical = 2.dp),
                        style = MaterialTheme.typography.labelMedium,
                        fontStyle = FontStyle.Italic,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                    )
                    return@items
                }
                Column {
                    Bubble(
                        m, c.theirReadUpTo,
                        // A later kind-2 notice for the same amount is this
                        // request being answered — the amount is the only
                        // thread from a payment back to the bill it settles.
                        // An identical re-bill will also read as paid, which
                        // errs the safe way for a button that spends.
                        paid = m.kind == 1 && !m.outgoing && messages.any {
                            it.kind == 2 && it.outgoing &&
                                it.amountPxmr == m.amountPxmr &&
                                it.timestamp >= m.timestamp
                        },
                        // The sender's own retract (kind 5, reOwn) withdraws
                        // the bill: same log, same seq, so the match is exact
                        // where the paid-check above can only go by amount.
                        cancelled = m.kind == 1 && messages.any {
                            it.kind == 5 && it.reOwn && it.reSeq == m.seq &&
                                it.outgoing == m.outgoing
                        },
                        onLongPress = { confirmDelete = m },
                        onPay = { billView = it },
                    )
                    val mine2 = reactions[Triple(m.outgoing, m.seq, true)]?.body
                    val theirs2 = reactions[Triple(m.outgoing, m.seq, false)]?.body
                    if (mine2 != null || theirs2 != null) {
                        Row(
                            Modifier.fillMaxWidth().padding(horizontal = 20.dp),
                            horizontalArrangement =
                                if (m.outgoing) Arrangement.End else Arrangement.Start,
                        ) {
                            Surface(
                                shape = MaterialTheme.shapes.small,
                                color = MaterialTheme.colorScheme.surfaceVariant,
                            ) {
                                Text(
                                    listOfNotNull(theirs2, mine2).joinToString(" "),
                                    style = MaterialTheme.typography.labelMedium,
                                    modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
                                )
                            }
                        }
                    }
                }
            }
        }
        }
    }

    // Nothing to fetch: their prekeys arrived with the handshake and live in
    // the contact record. §16.12's whole point is that the first message needs
    // no round trip to someone who may not be there.

    if (reserveOpen) {
        ReserveSheet(
            contact = c,
            onDone = { reserveOpen = false },
        )
    }

    billView?.let { b ->
        // The bill gets the whole screen (Ceremony.kt): a decision, not a
        // bubble. Accept leads to the same confirm screen as ever — §15.5
        // survives every coat of paint.
        BillScreen(
            m = b,
            contact = c,
            onPay = { billView = null; payRequest = b },
            onDecline = {
                billView = null
                scope.launch(Dispatchers.IO) {
                    runCatching {
                        Mailbox.send(
                            context, c,
                            context.getString(
                                R.string.chat_decline_bill, formatXmr(b.amountPxmr)
                            ),
                            mine,
                        )
                    }
                }
            },
            onClose = { billView = null },
        )
    }

    payRequest?.let { r ->
        // The contact rides along, not just the address. Paying a request as a
        // bare address silently dropped the payment notice — the vendor never
        // learned which transaction answered their bill, and nothing could be
        // marked paid. The request's own payto is already on the contact:
        // receiving a request stores it as their freshest address (§16.12).
        PaySheet(
            prefillContact = c,
            prefillAmountPxmr = r.amountPxmr,
        ) { payRequest = null }
    }

    // The same sheet the send/request button opens, with the contact already
    // chosen. A second, smaller money form in chat meant one place had a
    // currency switch and a number pad and the other did not, which is how the
    // unit someone is thinking in stops matching the unit they are typing in.
    if (askOpen) {
        PaySheet(prefillContact = c) { askOpen = false }
    }

    if (settingsOpen) {
        ChatSettingsDialog(
            current = store.disappearAfter(c.personaHex),
            onPick = { store.setDisappearAfter(c.personaHex, it); settingsOpen = false },
            onClearAll = {
                store.deleteThread(c.personaHex)
                messages = emptyList()
                settingsOpen = false
            },
            onDismiss = { settingsOpen = false },
        )
    }

    confirmDelete?.let { m ->
        AlertDialog(
            onDismissRequest = { confirmDelete = null },
            title = { Text(stringResource(R.string.chat_message_title)) },
            text = {
                Column {
                    if (m.kind != 4 && c.theirBundle != null) {
                        // React: one tap, one emoji, sent through the same
                        // sealed chain as everything else (§16.14).
                        Row(horizontalArrangement = Arrangement.SpaceEvenly,
                            modifier = Modifier.fillMaxWidth()) {
                            listOf("👍", "❤️", "😂", "😮", "😢", "🔥").forEach { emo ->
                                Text(
                                    emo,
                                    style = MaterialTheme.typography.headlineSmall,
                                    modifier = Modifier
                                        .clickable {
                                            confirmDelete = null
                                            scope.launch(Dispatchers.IO) {
                                                runCatching {
                                                    Mailbox.send(
                                                        context, c, emo, mine,
                                                        kind = 4,
                                                        reSeq = m.seq,
                                                        reOwn = m.outgoing,
                                                    )
                                                }.onSuccess {
                                                    messages = store.thread(c.personaHex)
                                                }.onFailure {
                                                    DucatLog.w("Chat", "react: ${it.message}")
                                                }
                                            }
                                        }
                                        .padding(4.dp),
                                )
                            }
                        }
                        Spacer(Modifier.height(12.dp))
                    }
                    Text(stringResource(R.string.chat_delete_note))
                }
            },
            confirmButton = {
                Row {
                    if (m.body.isNotBlank()) {
                        // Copy is how a card, an address, or a link gets passed
                        // along — forwarding by hand until forwarding exists.
                        TextButton(onClick = {
                            val cm = context.getSystemService(
                                android.content.ClipboardManager::class.java
                            )
                            cm?.setPrimaryClip(
                                android.content.ClipData.newPlainText("message", m.body)
                            )
                            confirmDelete = null
                        }) { Text(stringResource(R.string.chat_copy)) }
                    }
                    TextButton(onClick = {
                        store.deleteMessage(c.personaHex, m.seq, m.outgoing)
                        messages = store.thread(c.personaHex)
                        confirmDelete = null
                    }) {
                        Text(
                            stringResource(R.string.chat_delete),
                            color = MaterialTheme.colorScheme.error,
                        )
                    }
                }
            },
            dismissButton = {
                TextButton(onClick = { confirmDelete = null }) {
                    Text(stringResource(R.string.chat_cancel))
                }
            },
        )
    }
}

/**
 * Disappearing messages, stated honestly.
 *
 * This is a **local** retention rule. It cannot reach the other device, and a
 * screen implying otherwise would be worse than not offering it — the whole
 * point of §16.11 is not overstating what a guarantee covers.
 */
@Composable
private fun ChatSettingsDialog(
    current: Long,
    onPick: (Long) -> Unit,
    onClearAll: () -> Unit,
    onDismiss: () -> Unit,
) {
    val options = listOf(
        0L to stringResource(R.string.chat_keep_everything),
        3600L to stringResource(R.string.chat_1_hour),
        86_400L to stringResource(R.string.chat_1_day),
        604_800L to stringResource(R.string.chat_1_week),
    )
    var confirmClear by remember { mutableStateOf(false) }

    // Clearing a thread is unrecoverable and one tap from a settings sheet is
    // too close to it, so the tap opens a confirm rather than doing the delete.
    if (confirmClear) {
        AlertDialog(
            onDismissRequest = { confirmClear = false },
            title = { Text(stringResource(R.string.chat_clear_confirm_title)) },
            text = {
                Text(stringResource(R.string.chat_clear_confirm_text))
            },
            confirmButton = {
                TextButton(onClick = { confirmClear = false; onClearAll() }) {
                    Text(
                        stringResource(R.string.chat_clear),
                        color = MaterialTheme.colorScheme.error,
                    )
                }
            },
            dismissButton = {
                TextButton(onClick = { confirmClear = false }) {
                    Text(stringResource(R.string.chat_cancel))
                }
            },
        )
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.chat_conversation_title)) },
        text = {
            Column {
                Text(
                    stringResource(R.string.chat_delete_after),
                    style = MaterialTheme.typography.labelLarge,
                )
                Spacer(Modifier.height(8.dp))
                options.forEach { (secs, label) ->
                    Row(
                        Modifier.fillMaxWidth().clickable { onPick(secs) }.padding(vertical = 6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(selected = current == secs, onClick = { onPick(secs) })
                        Spacer(Modifier.width(8.dp))
                        Text(label)
                    }
                }
                Spacer(Modifier.height(10.dp))
                Text(
                    stringResource(R.string.chat_only_your_copy),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        },
        confirmButton = {
            TextButton(onClick = { confirmClear = true }) {
                Text(
                    stringResource(R.string.chat_clear_this_chat),
                    color = MaterialTheme.colorScheme.error,
                )
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(R.string.chat_done)) }
        },
    )
}

/** Seal, chain, append. Fails as a unit — nothing is stored unsent. */
private fun sendOne(
    context: android.content.Context,
    c: Contact,
    body: String,
    minePersonaHex: String,
): Contact = Mailbox.send(context, c, body, minePersonaHex)

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun Bubble(
    m: StoredMessage,
    theirReadUpTo: Long? = null,
    paid: Boolean = false,
    cancelled: Boolean = false,
    onLongPress: () -> Unit,
    onPay: (StoredMessage) -> Unit,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val align = if (m.outgoing) Alignment.End else Alignment.Start
    // Yours in the accent, theirs in the neutral surface. Colour is doing the
    // work here rather than alignment alone, because alignment is unreadable on
    // a narrow screen once messages wrap to full width.
    val bg = if (m.outgoing) MaterialTheme.colorScheme.primary
    else MaterialTheme.colorScheme.surfaceVariant
    val fg = if (m.outgoing) MaterialTheme.colorScheme.onPrimary
    else MaterialTheme.colorScheme.onSurfaceVariant
    val corner = if (m.outgoing) {
        RoundedCornerShape(16.dp, 16.dp, 4.dp, 16.dp)
    } else {
        RoundedCornerShape(16.dp, 16.dp, 16.dp, 4.dp)
    }

    Column(Modifier.fillMaxWidth(), horizontalAlignment = align) {
        Box(
            Modifier
                .widthIn(max = 280.dp)
                .background(bg, corner)
                .combinedClickable(onClick = {}, onLongClick = onLongPress)
                .padding(horizontal = 14.dp, vertical = 10.dp)
        ) {
            if (m.kind == 0) {
                val ctx = LocalContext.current
                val att = m.attHash
                if (att != null) {
                    val file = remember(att) { Mailbox.attachmentFile(ctx, att) }
                    val mime = m.attMime ?: "application/octet-stream"
                    when {
                        !file.exists() -> Text(
                            when {
                                mime.startsWith("image/") ->
                                    stringResource(R.string.chat_downloading_image)
                                mime.startsWith("audio/") ->
                                    stringResource(R.string.chat_downloading_audio)
                                else -> stringResource(R.string.chat_downloading_file)
                            },
                            color = fg.copy(alpha = 0.8f),
                            style = MaterialTheme.typography.bodySmall,
                        )
                        mime.startsWith("image/") -> {
                            val bmp = remember(att) {
                                runCatching {
                                    // Bounded decode: the protocol capped the
                                    // bytes, but the pixels are still the
                                    // decoder's problem.
                                    val o = android.graphics.BitmapFactory.Options()
                                        .apply { inSampleSize = 1 }
                                    android.graphics.BitmapFactory
                                        .decodeFile(file.absolutePath, o)
                                }.getOrNull()
                            }
                            if (bmp != null) {
                                androidx.compose.foundation.Image(
                                    bmp.asImageBitmap(), stringResource(R.string.chat_picture_desc),
                                    modifier = Modifier
                                        .widthIn(max = 240.dp)
                                        .clip(MaterialTheme.shapes.medium),
                                    contentScale = androidx.compose.ui.layout.ContentScale.Fit,
                                )
                            } else {
                                Text(
                                    stringResource(R.string.chat_could_not_decode),
                                    color = fg.copy(alpha = 0.8f),
                                )
                            }
                        }
                        mime.startsWith("audio/") -> AudioBubble(file, fg)
                        else -> FileBubble(
                            file,
                            m.attName ?: stringResource(R.string.chat_file_fallback),
                            m.attLen, mime, fg,
                        )
                    }
                    if (m.body.isNotBlank() && m.body !in setOf("📷", "🎤") &&
                        m.body != "📎 ${m.attName}"
                    ) {
                        Spacer(Modifier.height(4.dp))
                        Text(m.body, color = fg)
                    }
                } else {
                    // A card in a chat is usually an in-person introduction:
                    // the third party is standing right there, and their scan
                    // of this screen is the same scan as any other code. The
                    // link stays tappable for when they are not.
                    val cardUri = remember(m.body) {
                        Regex("ducat:\\S+").find(m.body)?.value
                    }
                    if (cardUri != null) {
                        Column {
                            Box(Modifier.clip(MaterialTheme.shapes.medium)) {
                                QrBlock(cardUri)
                            }
                            Spacer(Modifier.height(6.dp))
                            LinkableText(m.body, fg)
                        }
                    } else {
                        LinkableText(m.body, fg)
                    }
                }
            } else {
                Column {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(
                            when {
                                m.kind == 1 -> Icons.Filled.RequestQuote
                                m.kind == 3 -> Icons.Filled.Receipt
                                m.kind == 6 || m.kind == 7 -> Icons.Filled.DirectionsCar
                                // A payment points the way the money went, the
                                // same as the Activity tab draws it. An incoming
                                // one used to get the outgoing arrow, so the
                                // picture argued with the words beside it.
                                m.outgoing -> Icons.Filled.ArrowUpward
                                else -> Icons.Filled.ArrowDownward
                            },
                            null,
                            Modifier.size(14.dp),
                            tint = fg.copy(alpha = 0.8f),
                        )
                        Spacer(Modifier.width(4.dp))
                        Text(
                            when {
                                m.kind == 1 && m.outgoing ->
                                    stringResource(R.string.chat_you_asked_for)
                                m.kind == 1 -> stringResource(R.string.chat_asked_you_for)
                                // A receipt is issued by whoever *received* the
                                // money, so the direction reads the other way
                                // round from a notice.
                                m.kind == 3 && m.outgoing ->
                                    stringResource(R.string.chat_receipt_you_issued)
                                m.kind == 3 -> stringResource(R.string.chat_receipt)
                                m.kind == 6 && m.outgoing ->
                                    stringResource(R.string.chat_you_offered_to_drive)
                                m.kind == 6 -> stringResource(R.string.chat_offers_to_drive_you)
                                m.kind == 7 && m.outgoing ->
                                    stringResource(R.string.chat_you_accepted_ride)
                                m.kind == 7 -> stringResource(R.string.chat_ride_accepted)
                                m.outgoing -> stringResource(R.string.chat_you_sent)
                                else -> stringResource(R.string.chat_sent_you)
                            },
                            style = MaterialTheme.typography.labelSmall,
                            color = fg.copy(alpha = 0.8f),
                        )
                    }
                    val a = Amounts.show(context, m.amountPxmr)
                    Text(
                        a.primary,
                        style = MaterialTheme.typography.titleMedium,
                        color = fg,
                    )
                    a.secondary?.let {
                        Text(it, style = MaterialTheme.typography.labelSmall,
                             color = fg.copy(alpha = 0.75f))
                    }
                    if (m.kind == 6) m.etaSecs?.let { secs ->
                        Text(
                            stringResource(R.string.chat_min_away, (secs / 60).coerceAtLeast(1)),
                            style = MaterialTheme.typography.labelSmall,
                            color = fg.copy(alpha = 0.8f),
                        )
                    }
                    if (m.body.isNotBlank()) {
                        Spacer(Modifier.height(2.dp))
                        Text(m.body, color = fg, style = MaterialTheme.typography.bodySmall)
                    }
                    if (m.items.isNotEmpty()) {
                        Spacer(Modifier.height(8.dp))
                        Bill(m, fg)
                    }
                    // §16.13: a request carries no authority. An incoming one
                    // that offered a one-tap "pay" would be exactly the shortcut
                    // §15.5's confirm screen exists to prevent.
                    if (m.kind == 1 && !m.outgoing) {
                        Spacer(Modifier.height(8.dp))
                        if (paid) {
                            // Settled: a live button here would offer to pay
                            // the same bill twice.
                            Text(
                                stringResource(R.string.chat_paid),
                                style = MaterialTheme.typography.labelMedium,
                                color = fg.copy(alpha = 0.8f),
                            )
                        } else if (cancelled) {
                            // Withdrawn by its sender: a live button here
                            // would offer to pay money nobody is watching
                            // for (§15.11).
                            Text(
                                stringResource(R.string.chat_cancelled),
                                style = MaterialTheme.typography.labelMedium,
                                color = fg.copy(alpha = 0.8f),
                            )
                        } else if (m.payto != null) {
                            // Opens the send screen filled in. It does **not**
                            // pay: §16.13 forbids a one-tap spend from an
                            // arriving message, because the confirm screen is
                            // the only thing between a message and money
                            // leaving.
                            FilledTonalButton(
                                onClick = { onPay(m) },
                                contentPadding = PaddingValues(horizontal = 12.dp, vertical = 4.dp),
                            ) {
                                Text(
                                    stringResource(R.string.chat_review_payment),
                                    style = MaterialTheme.typography.labelMedium,
                                )
                            }
                        } else {
                            Text(
                                stringResource(R.string.chat_no_address),
                                style = MaterialTheme.typography.labelSmall,
                                color = fg.copy(alpha = 0.7f),
                            )
                        }
                    }
                }
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (!m.forwardSecret) {
                // §16.11: the signed-prekey fallback is a real weakening and is
                // surfaced rather than swallowed.
                Icon(
                    Icons.Filled.LockOpen,
                    stringResource(R.string.chat_no_forward_secrecy),
                    Modifier.size(12.dp),
                    tint = MaterialTheme.colorScheme.error,
                )
                Spacer(Modifier.width(4.dp))
            }
            if (m.outgoing) {
                // §16.16: their watermark, when they publish one. Their claim,
                // shown as one — a tick, not a certainty. No watermark, no
                // ticks: absence of the feature is not "unread".
                theirReadUpTo?.let { read ->
                    Text(
                        if (read > m.seq) "✓✓" else "✓",
                        style = MaterialTheme.typography.labelSmall,
                        color = if (read > m.seq) MaterialTheme.ducat.settled
                        else MaterialTheme.colorScheme.outline,
                    )
                    Spacer(Modifier.width(4.dp))
                }
            }
            Text(
                SimpleDateFormat("HH:mm", Locale.getDefault())
                    .format(Date(m.timestamp * 1000)),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.outline,
            )
        }
    }
}

/**
 * Body text, with any link in it made tappable.
 *
 * A location message is a link (a map anyone can open beats a coordinate
 * format only this app understands), so the text bubble has to honour links
 * or the feature reads as broken. `ducat:` rides the same path: a card passed
 * along in a chat is claimed by tapping it, exactly as it would be from a
 * browser — the VIEW intent lands back in this app's claim flow.
 */
@Composable
private fun LinkableText(body: String, fg: androidx.compose.ui.graphics.Color) {
    val context = LocalContext.current
    val url = remember(body) {
        Regex("(https://|ducat:)\\S+").find(body)?.value
    }
    if (url == null) {
        Text(body, color = fg)
    } else {
        Text(
            body,
            color = fg,
            textDecoration = androidx.compose.ui.text.style.TextDecoration.Underline,
            modifier = Modifier.clickable {
                runCatching {
                    context.startActivity(
                        android.content.Intent(
                            android.content.Intent.ACTION_VIEW,
                            android.net.Uri.parse(url),
                        )
                    )
                }
            },
        )
    }
}

/** A voice memo: play and stop, nothing more. The bytes are already local. */
@Composable
private fun AudioBubble(file: java.io.File, fg: androidx.compose.ui.graphics.Color) {
    var playing by remember { mutableStateOf(false) }
    val player = remember { android.media.MediaPlayer() }
    DisposableEffect(Unit) { onDispose { runCatching { player.release() } } }
    Row(verticalAlignment = Alignment.CenterVertically) {
        IconButton(onClick = {
            if (playing) {
                runCatching { player.stop() }
                playing = false
            } else {
                runCatching {
                    player.reset()
                    player.setDataSource(file.absolutePath)
                    player.setOnCompletionListener { playing = false }
                    player.prepare()
                    player.start()
                }.onSuccess { playing = true }
            }
        }) {
            Icon(
                if (playing) Icons.Filled.Stop else Icons.Filled.PlayArrow,
                if (playing) stringResource(R.string.chat_stop)
                else stringResource(R.string.chat_play_voice_memo),
                tint = fg,
            )
        }
        Text(
            stringResource(R.string.chat_voice_memo),
            color = fg, style = MaterialTheme.typography.bodyMedium,
        )
    }
}

/** Any other file: name and size, tap to open with whatever handles its type. */
@Composable
private fun FileBubble(
    file: java.io.File,
    name: String,
    len: Long,
    mime: String,
    fg: androidx.compose.ui.graphics.Color,
) {
    val context = LocalContext.current
    Row(
        Modifier.clickable {
            runCatching {
                val uri = androidx.core.content.FileProvider.getUriForFile(
                    context, context.packageName + ".backups", file,
                )
                context.startActivity(
                    android.content.Intent(android.content.Intent.ACTION_VIEW)
                        .setDataAndType(uri, mime)
                        .addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION)
                )
            }.onFailure { DucatLog.w("Chat", "open file: ${it.message}") }
        },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Icons.Filled.InsertDriveFile, null, Modifier.size(28.dp), tint = fg)
        Spacer(Modifier.width(8.dp))
        Column {
            Text(name, color = fg, style = MaterialTheme.typography.bodyMedium)
            Text(
                if (len >= 1024) stringResource(R.string.chat_size_kb, len / 1024)
                else stringResource(R.string.chat_size_b, len),
                color = fg.copy(alpha = 0.7f),
                style = MaterialTheme.typography.labelSmall,
            )
        }
    }
}

/**
 * The breakdown, as a bill reads on paper.
 *
 * Monospace and right-aligned amounts, because a column of numbers that does
 * not line up is a column nobody checks. And checking is the point: core
 * refuses a message whose items and tax do not equal its amount (§16.13), so
 * what is drawn here always adds up — the reader can confirm it by eye, which
 * is a different and better thing from being told to trust it.
 *
 * The network fee is deliberately absent. A Monero fee is paid by the sender to
 * the network, not by the payer to the vendor, so a fee line on a vendor's bill
 * charges it twice: once in the total asked for and again when the payer's own
 * wallet builds the transaction. What the transfer cost is on the payer's
 * Activity screen, from their own record, which is the only place it can be
 * stated truthfully.
 */
@Composable
private fun Bill(m: StoredMessage, fg: androidx.compose.ui.graphics.Color) {
    val context = LocalContext.current
    val subtotal = m.items.sumOf { it.amountPxmr }

    @Composable
    fun line(label: String, pxmr: Long, strong: Boolean = false) {
        Row(Modifier.fillMaxWidth()) {
            Text(
                label,
                style = MaterialTheme.typography.labelSmall,
                color = fg.copy(alpha = if (strong) 1f else 0.85f),
                fontWeight = if (strong) FontWeight.SemiBold else FontWeight.Normal,
                modifier = Modifier.weight(1f),
            )
            Spacer(Modifier.width(8.dp))
            Text(
                formatXmr(pxmr),
                style = MaterialTheme.typography.labelSmall,
                fontFamily = FontFamily.Monospace,
                color = fg.copy(alpha = if (strong) 1f else 0.85f),
                fontWeight = if (strong) FontWeight.SemiBold else FontWeight.Normal,
            )
        }
    }

    Column(
        Modifier
            .background(fg.copy(alpha = 0.08f), RoundedCornerShape(8.dp))
            .padding(10.dp)
    ) {
        m.items.forEach { line(it.description, it.amountPxmr) }
        if (m.taxPxmr != null) {
            Spacer(Modifier.height(4.dp))
            HorizontalDivider(color = fg.copy(alpha = 0.25f))
            Spacer(Modifier.height(4.dp))
            line(stringResource(R.string.chat_subtotal), subtotal)
            line(stringResource(R.string.chat_tax), m.taxPxmr)
        }
        Spacer(Modifier.height(4.dp))
        HorizontalDivider(color = fg.copy(alpha = 0.25f))
        Spacer(Modifier.height(4.dp))
        line(stringResource(R.string.chat_total), m.amountPxmr, strong = true)
        Amounts.show(context, m.amountPxmr).secondary?.let {
            Text(
                it,
                style = MaterialTheme.typography.labelSmall,
                color = fg.copy(alpha = 0.7f),
                modifier = Modifier.align(Alignment.End),
            )
        }
    }
}

/**
 * Resize, seal, park, reference (§16.15).
 *
 * Re-encoded rather than passed through, same as avatars: the picker hands
 * back an arbitrary file, and what goes out must be something this device's
 * own image stack produced. Quality steps down until the ciphertext fits one
 * record — 32 chunks of 32 KiB is Veilid's cap, and a picture that misses it
 * is refused on arrival, so better to lose detail here.
 */
private fun sendPicture(
    context: android.content.Context,
    c: Contact,
    mine: String,
    uri: android.net.Uri,
) {
    val src = context.contentResolver.openInputStream(uri).use {
        android.graphics.BitmapFactory.decodeStream(it)
    } ?: throw IllegalArgumentException(context.getString(R.string.chat_not_an_image))
    val maxDim = 1280
    val scale = minOf(1f, maxDim.toFloat() / maxOf(src.width, src.height))
    val scaled = if (scale < 1f) android.graphics.Bitmap.createScaledBitmap(
        src, (src.width * scale).toInt(), (src.height * scale).toInt(), true,
    ) else src

    var plain: ByteArray? = null
    for (q in intArrayOf(85, 70, 55, 40)) {
        val out = java.io.ByteArrayOutputStream()
        scaled.compress(android.graphics.Bitmap.CompressFormat.JPEG, q, out)
        val b = out.toByteArray()
        if (b.size <= 900_000) { plain = b; break }
    }
    val bytes = plain ?: throw IllegalArgumentException(
        context.getString(R.string.chat_picture_too_big)
    )
    sendAttachmentBytes(context, c, mine, bytes, "image/jpeg", null, "📷")
}

/**
 * Seal, park, reference, send — the §16.15 tail every attachment shares.
 *
 * A picture, a voice memo and a file differ only in how their bytes came to
 * exist; from here down they are identical: sealed under a fresh key, chunked
 * into a record of their own, referenced from a sealed message, and cached
 * locally under the ciphertext hash so the sender's bubble never says
 * "downloading" about its own bytes.
 */
private fun sendAttachmentBytes(
    context: android.content.Context,
    c: Contact,
    mine: String,
    bytes: ByteArray,
    mime: String,
    name: String?,
    body: String,
) {
    val rng = java.security.SecureRandom()
    val key = ByteArray(32).also(rng::nextBytes)
    val nonce = ByteArray(24).also(rng::nextBytes)
    val ct = uniffi.ducat_mobile.attachmentSeal(key, nonce, bytes)
    val hash = java.security.MessageDigest.getInstance("SHA-256").digest(ct)

    // One record, ciphertext chunked across its subkeys.
    val chunks = (ct.size + 32_767) / 32_768
    val rec = uniffi.ducat_mobile.nodeDhtCreate(chunks.toUInt())
    for (i in 0 until chunks) {
        val end = minOf((i + 1) * 32_768, ct.size)
        uniffi.ducat_mobile.nodeDhtSet(rec.key, i.toUInt(), ct.copyOfRange(i * 32_768, end))
    }

    val ref = uniffi.ducat_mobile.AttachmentRef(
        recordKey = rec.key,
        key = key, nonce = nonce,
        len = bytes.size.toULong(),
        ctHash = hash,
        mime = mime,
        name = name,
    )
    Mailbox.send(context, c, body, mine, attachment = ref)
    Mailbox.attachmentFile(context, hash.joinToString("") { "%02x".format(it) })
        .writeBytes(bytes)
}

/** The record cap: 32 subkeys of 32 KiB, minus the AEAD tag's 16 bytes. */
private const val MAX_FILE_BYTES = 32 * 32_768 - 16

/** A voice memo: the recorder's m4a, sent as an ordinary attachment. */
private fun sendVoice(
    context: android.content.Context,
    c: Contact,
    mine: String,
    memo: java.io.File,
) {
    try {
        val bytes = memo.readBytes()
        if (bytes.isEmpty()) {
            throw IllegalArgumentException(context.getString(R.string.chat_nothing_recorded))
        }
        if (bytes.size > MAX_FILE_BYTES) {
            throw IllegalArgumentException(context.getString(R.string.chat_memo_too_long))
        }
        // The recorder names the format it actually produced: a phone's is
        // AAC in MP4, a desk's is WAV, because a JVM ships no AAC encoder.
        // Labelling by extension rather than by assumption is what lets a
        // memo recorded on either one play on the other.
        val wav = memo.extension.equals("wav", ignoreCase = true)
        sendAttachmentBytes(
            context, c, mine, bytes,
            if (wav) "audio/wav" else "audio/mp4",
            if (wav) "Voice memo.wav" else "Voice memo.m4a",
            "🎤",
        )
    } finally {
        memo.delete()
    }
}

/** Any file the picker hands over, if it fits one record. */
private fun sendFile(
    context: android.content.Context,
    c: Contact,
    mine: String,
    uri: android.net.Uri,
) {
    val resolver = context.contentResolver
    val name = resolver.query(uri, null, null, null, null)?.use { cur ->
        val i = cur.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
        if (i >= 0 && cur.moveToFirst()) cur.getString(i) else null
    } ?: context.getString(R.string.chat_file_fallback)
    val bytes = resolver.openInputStream(uri)?.use { it.readBytes() }
        ?: throw IllegalArgumentException(context.getString(R.string.chat_could_not_read_file))
    if (bytes.size > MAX_FILE_BYTES) {
        throw IllegalArgumentException(
            context.getString(R.string.chat_file_too_big, bytes.size / 1024)
        )
    }
    val mime = resolver.getType(uri) ?: "application/octet-stream"
    sendAttachmentBytes(context, c, mine, bytes, mime, name, "📎 $name")
}

/**
 * Hold-to-record (§16.15's bytes, Signal's gesture).
 *
 * AAC in MP4 at 32 kbps: universally decodable, and a minute of speech lands
 * near 250 KB — comfortably one record. A press too short to be speech is
 * discarded rather than sent, so brushing the icon costs nothing.
 */
private class VoiceRecorder(private val context: android.content.Context) {
    private var rec: android.media.MediaRecorder? = null
    private var file: java.io.File? = null
    private var startedAt = 0L

    fun start(): Boolean = runCatching {
        val f = voiceMemoFile(context)
        @Suppress("DEPRECATION")
        val r = android.media.MediaRecorder()
        r.setAudioSource(android.media.MediaRecorder.AudioSource.MIC)
        r.setOutputFormat(android.media.MediaRecorder.OutputFormat.MPEG_4)
        r.setAudioEncoder(android.media.MediaRecorder.AudioEncoder.AAC)
        r.setAudioEncodingBitRate(32_000)
        r.setAudioSamplingRate(44_100)
        r.setOutputFile(f.absolutePath)
        r.prepare()
        r.start()
        rec = r
        file = f
        startedAt = System.currentTimeMillis()
    }.onFailure {
        DucatLog.w("Chat", "recorder: ${it.message}")
        rec?.release(); rec = null
    }.isSuccess

    /** Null when the take was too short or the recorder failed. */
    fun stop(): java.io.File? {
        val r = rec ?: return null
        rec = null
        val clean = runCatching { r.stop() }.isSuccess
        r.release()
        val f = file
        file = null
        val longEnough = System.currentTimeMillis() - startedAt >= 700
        return if (clean && longEnough && f != null && f.length() > 0) {
            f
        } else {
            f?.delete()
            null
        }
    }
}


/**
 * A contact, shared as the readable claim it is (§16.2).
 *
 * What travels is the profile the contact *asserted* — name, persona, and any
 * reachability they chose to publish — not a connection ticket: a card is
 * claim-once and minting one for a third party is not this device's to do.
 * The persona key is the durable part; everything else is introduction.
 */
private fun contactCard(c: Contact): String = buildString {
    append("👤 ${c.displayName()}\n")
    c.email?.let { append("✉ $it\n") }
    c.phone?.let { append("☎ $it\n") }
    c.signal?.let { append("Signal: $it\n") }
    append("DUCAT persona: ${c.personaHex}")
}

@Composable
private fun TrayItem(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    label: String,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        FilledTonalIconButton(
            onClick = onClick,
            enabled = enabled,
            modifier = Modifier.size(52.dp),
        ) { Icon(icon, label) }
        Text(label, style = MaterialTheme.typography.labelSmall)
    }
}

/**
 * Two different things a person means by "share a contact", kept honest.
 *
 * **A card for me** is the introduction: a fresh claim-once card minted here,
 * dropped into the thread for the other side to pass along. It is the only
 * connectable thing this device is entitled to mint — nobody can be made
 * reachable except by their own device issuing a card.
 *
 * **Someone's profile** is exactly what it says: the name and reachability
 * that contact chose to publish, as text. Useful for "here's the bartender's
 * Signal", and deliberately not a connection code, because those are theirs
 * to give.
 */
@Composable
private fun ContactPickDialog(
    contacts: List<Contact>,
    onIntroduceMe: () -> Unit,
    onPick: (Contact) -> Unit,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.chat_share)) },
        text = {
            Column {
                Row(
                    Modifier.fillMaxWidth().clickable(onClick = onIntroduceMe)
                        .padding(vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("🎟", style = MaterialTheme.typography.headlineSmall)
                    Spacer(Modifier.width(12.dp))
                    Column {
                        Text(
                            stringResource(R.string.chat_card_for_me),
                            style = MaterialTheme.typography.bodyLarge,
                        )
                        Text(
                            stringResource(R.string.chat_card_for_me_desc),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                if (contacts.isNotEmpty()) {
                    Spacer(Modifier.height(10.dp))
                    Text(
                        stringResource(R.string.chat_someones_profile),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(Modifier.height(4.dp))
                    contacts.forEach { c ->
                        Row(
                            Modifier.fillMaxWidth().clickable { onPick(c) }
                                .padding(vertical = 8.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Avatar(c.displayName(), c.avatar)
                            Spacer(Modifier.width(12.dp))
                            Text(c.displayName(), style = MaterialTheme.typography.bodyLarge)
                        }
                    }
                }
            }
        },
        confirmButton = {},
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(R.string.chat_cancel)) }
        },
    )
}

/**
 * The ride's escrow, as one quiet banner above the thread (§15.12).
 *
 * Every stage shows the one thing that can happen next, and only to the
 * party who can do it: the rider funds and releases, the driver completes.
 * The stage lives in the ceremony store and every device derives its own
 * view of it — nothing here is authority, the FROST signatures are.
 */
@Composable
private fun RideBondBanner(contact: Contact) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val ride = remember(version, contact.personaHex) {
        org.ducatproject.ducat.Ceremony.rideWith(context, contact.personaHex)
    } ?: return
    val stage = ride.optString("stage")
    val fare = ride.optLong("farePxmr")
    val funded = ride.optLong("fundedPxmr")
    val rider = org.ducatproject.ducat.Ceremony.isFunder(ride)
    val reservation = ride.optInt("kind") == org.ducatproject.ducat.Ceremony.KIND_RESERVATION
    // What "secured" means: a ride waits for the fare; a reservation for
    // rent plus both deposits — the host's included, because their funding
    // is their acceptance.
    val need = if (reservation) org.ducatproject.ducat.Ceremony.expectedTotalPxmr(ride) else fare
    val idHex = ride.optString("id")
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var countering by remember { mutableStateOf(false) }
    var counterXmr by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()

    // Nudge the mail and, while the escrow waits for its money, ask the
    // chain. The global poller would get there; a ride at a curb wants
    // seconds. Funding checks every third tick — a scan is a real RPC.
    LaunchedEffect(idHex, stage, funded) {
        if (stage == "released" || stage == "release_cosigned") return@LaunchedEffect
        var tick = 0
        while (true) {
            kotlinx.coroutines.delay(3_000)
            tick++
            withContext(Dispatchers.IO) {
                runCatching { Mailbox.poll(context) }
                if (stage == "done" && funded < need && tick % 3 == 0) {
                    runCatching {
                        org.ducatproject.ducat.Ceremony.checkRideFunding(context, idHex)
                    }
                }
            }
        }
    }

    Surface(
        color = MaterialTheme.colorScheme.secondaryContainer,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 10.dp)) {
            val fareShown = Amounts.show(context, fare).primary
            when {
                stage == "committed" || stage == "shared" -> {
                    BondLine(spin = true, text = stringResource(R.string.bond_building, fareShown))
                }
                // The exposed side goes second. Whoever funds first stands
                // alone until the other follows, and the payer is carrying
                // ten times what the provider is — so when a provider stake
                // was asked for, the payer waits for it. It also matches how
                // booking anything works: the other side confirms, then you
                // pay. A provider who never stakes has simply declined, and
                // nobody's money is sitting in a shared address over it.
                // Waiting on the other side's stake — measured by this
                // device's own scan of the escrow, not by their say-so.
                // `hostFundTxid` is written on *their* device when they pay;
                // it never reaches here, so waiting on it would wait forever.
                // The chain is the shared fact, which is the same rule that
                // makes "secured" a fact rather than a claim (§17.5).
                stage == "done" && rider && ride.optString("fundTxid").isEmpty() &&
                    ride.optLong("hostDepPxmr") > 0 &&
                    funded < ride.optLong("hostDepPxmr") ->
                    BondLine(spin = true, text = stringResource(R.string.bond_await_their_stake))

                stage == "done" && rider && ride.optString("fundTxid").isEmpty() -> {
                    BondLine(spin = false, text = stringResource(R.string.bond_escrow_ready))
                    Spacer(Modifier.height(6.dp))
                    // Fare plus margin: the margin comes home in the release,
                    // and is what makes releasing beat sulking when there is
                    // no arbiter to appeal to.
                    val fundShown = Amounts.show(
                        context, org.ducatproject.ducat.Ceremony.mySharePxmr(ride),
                    ).primary
                    Button(
                        onClick = {
                            busy = true; error = null
                            scope.launch {
                                withContext(Dispatchers.IO) {
                                    runCatching {
                                        org.ducatproject.ducat.Ceremony.fundRide(context, idHex)
                                    }
                                }.onFailure { error = it.message }
                                busy = false
                            }
                        },
                        enabled = !busy,
                        modifier = Modifier.fillMaxWidth().height(44.dp),
                    ) { Text(stringResource(R.string.bond_secure_fare, fundShown)) }
                    // The promise, at the moment the money is asked for,
                    // rather than in a help page nobody opens.
                    val myStake = org.ducatproject.ducat.Stakes.stakeFor(
                        if (reservation) org.ducatproject.ducat.Stakes.Deal.Stay
                        else org.ducatproject.ducat.Stakes.Deal.Ride,
                        ride.optLong("farePxmr"),
                    )
                    if (myStake > 0) {
                        Spacer(Modifier.height(4.dp))
                        BondNote(
                            stringResource(
                                R.string.bond_stake_refunded,
                                Amounts.show(context, myStake).primary,
                            ),
                        )
                    }
                }
                stage == "done" && rider && funded < need ->
                    BondLine(spin = true, text = stringResource(R.string.bond_fare_sent))
                stage == "done" && rider -> {
                    BondLine(spin = false, text = stringResource(
                        if (reservation) R.string.res_secured else R.string.bond_fare_secured))
                    if (ride.optInt("arbiterIdx") != 0) {
                        Spacer(Modifier.height(4.dp))
                        OutlinedButton(
                            onClick = {
                                busy = true; error = null
                                scope.launch {
                                    withContext(Dispatchers.IO) {
                                        runCatching {
                                            // The stranded rider asks for
                                            // everything back; the arbiter
                                            // judges, and can decline by
                                            // simply not signing.
                                            org.ducatproject.ducat.Ceremony.proposeRideSplit(
                                                context, idHex, funded, toArbiter = true)
                                        }
                                    }.onFailure { error = it.message }
                                    busy = false
                                }
                            },
                            enabled = !busy,
                            modifier = Modifier.fillMaxWidth().height(40.dp),
                        ) { Text(stringResource(R.string.bond_ask_arbiter)) }
                    }
                }
                // Whoever owes this escrow something and has not sent it:
                // a reservation's host accepting by funding their deposit,
                // or a driver posting a stake on a two-sided ride. Same
                // gesture, same button, because it is the same act — money
                // in, which is the only acceptance that means anything.
                stage == "done" && !rider &&
                    org.ducatproject.ducat.Ceremony.mySharePxmr(ride) > 0 &&
                    ride.optString("hostFundTxid").isEmpty() -> {
                    BondLine(
                        spin = false,
                        text = stringResource(
                            if (reservation) R.string.res_proposed else R.string.bond_stake_asked,
                        ),
                    )
                    Spacer(Modifier.height(6.dp))
                    val myShown = Amounts.show(
                        context, org.ducatproject.ducat.Ceremony.mySharePxmr(ride),
                    ).primary
                    Button(
                        onClick = {
                            busy = true; error = null
                            scope.launch {
                                withContext(Dispatchers.IO) {
                                    runCatching {
                                        org.ducatproject.ducat.Ceremony.fundRide(context, idHex)
                                    }
                                }.onFailure { error = it.message }
                                busy = false
                            }
                        },
                        enabled = !busy,
                        modifier = Modifier.fillMaxWidth().height(44.dp),
                    ) {
                        Text(
                            stringResource(
                                if (reservation) R.string.res_accept_fund
                                else R.string.bond_post_stake,
                                myShown,
                            ),
                        )
                    }
                    Spacer(Modifier.height(4.dp))
                    BondNote(stringResource(R.string.bond_stake_refunded, myShown))
                }
                stage == "done" && !rider && funded < need ->
                    BondLine(spin = true, text = stringResource(R.string.bond_waiting_funding))
                stage == "done" && !rider -> {
                    BondLine(spin = false, text = stringResource(
                        if (reservation) R.string.res_secured else R.string.bond_fare_secured))
                    Spacer(Modifier.height(6.dp))
                    Button(
                        onClick = {
                            busy = true; error = null
                            scope.launch {
                                withContext(Dispatchers.IO) {
                                    runCatching {
                                        org.ducatproject.ducat.Ceremony
                                            .proposeRideRelease(context, idHex)
                                    }
                                }.onFailure { error = it.message }
                                busy = false
                            }
                        },
                        enabled = !busy,
                        modifier = Modifier.fillMaxWidth().height(44.dp),
                    ) { Text(stringResource(
                        if (reservation) R.string.res_settle else R.string.bond_complete_ride)) }
                }
                stage == "releasing" -> {
                    BondLine(spin = true, text = stringResource(R.string.bond_waiting_release))
                    // §9.3: the counterparty gone, the arbiter is the way out.
                    // Same proposal, different signer; their signature is the
                    // ruling.
                    if (ride.optInt("arbiterIdx") != 0) {
                        Spacer(Modifier.height(4.dp))
                        OutlinedButton(
                            onClick = {
                                busy = true; error = null
                                val back = ride.optLong("myRiderBack",
                                    (funded - fare).coerceAtLeast(0L))
                                scope.launch {
                                    withContext(Dispatchers.IO) {
                                        runCatching {
                                            org.ducatproject.ducat.Ceremony.proposeRideSplit(
                                                context, idHex, back, toArbiter = true)
                                        }
                                    }.onFailure { error = it.message }
                                    busy = false
                                }
                            },
                            enabled = !busy,
                            modifier = Modifier.fillMaxWidth().height(40.dp),
                        ) { Text(stringResource(R.string.bond_ask_arbiter)) }
                    }
                    val mine = ride.optLong("myRiderBack", -1L)
                    if (mine >= 0) {
                        Spacer(Modifier.height(2.dp))
                        Text(
                            stringResource(
                                R.string.bond_split_stated,
                                Amounts.show(context, mine).primary,
                                Amounts.show(context, (funded - mine).coerceAtLeast(0L)).primary,
                            ),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSecondaryContainer,
                        )
                    }
                    // The proposer can re-propose: a broadcast can die on the
                    // node, and a fresh proposal (new nonces, same inputs) is
                    // the retry. The rider is simply asked for their yes again.
                    if (!rider) {
                        Spacer(Modifier.height(6.dp))
                        OutlinedButton(
                            onClick = {
                                busy = true; error = null
                                scope.launch {
                                    withContext(Dispatchers.IO) {
                                        runCatching {
                                            org.ducatproject.ducat.Ceremony
                                                .proposeRideRelease(context, idHex)
                                        }
                                    }.onFailure { error = it.message }
                                    busy = false
                                }
                            },
                            enabled = !busy,
                            modifier = Modifier.fillMaxWidth().height(40.dp),
                        ) { Text(stringResource(R.string.bond_complete_ride)) }
                    }
                }
                stage == "release_pending" -> {
                    // A proposal stands, from the other side (either side may
                    // propose — §15.12's settlement). State the claimed split
                    // and offer the only two moves: sign it, or counter.
                    val riderBack = ride.optLong("pendingRiderBack", (funded - fare).coerceAtLeast(0L))
                    val toDriver = (funded - riderBack).coerceAtLeast(0L)
                    BondLine(spin = false, text = stringResource(R.string.bond_ride_complete_ask))
                    Spacer(Modifier.height(2.dp))
                    Text(
                        stringResource(
                            R.string.bond_split_stated,
                            Amounts.show(context, riderBack).primary,
                            Amounts.show(context, toDriver).primary,
                        ),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSecondaryContainer,
                    )
                    Spacer(Modifier.height(6.dp))
                    Button(
                        onClick = {
                            busy = true; error = null
                            scope.launch {
                                withContext(Dispatchers.IO) {
                                    runCatching {
                                        org.ducatproject.ducat.Ceremony
                                            .approveRideRelease(context, idHex)
                                    }
                                }.onFailure { error = it.message }
                                busy = false
                            }
                        },
                        enabled = !busy,
                        modifier = Modifier.fillMaxWidth().height(44.dp),
                    ) { Text(stringResource(R.string.bond_sign_split)) }
                    // The counter: one number — what goes back to the rider —
                    // and a fresh proposal supersedes theirs, roles swapped.
                    Spacer(Modifier.height(4.dp))
                    if (!countering) {
                        OutlinedButton(
                            onClick = { countering = true },
                            enabled = !busy,
                            modifier = Modifier.fillMaxWidth().height(40.dp),
                        ) { Text(stringResource(R.string.bond_propose_split)) }
                    } else {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            OutlinedTextField(
                                value = counterXmr,
                                onValueChange = {
                                    counterXmr = it.filter { c -> c.isDigit() || c == '.' }
                                },
                                label = { Text(stringResource(R.string.bond_back_to_rider)) },
                                singleLine = true,
                                modifier = Modifier.weight(1f),
                            )
                            Spacer(Modifier.width(6.dp))
                            Button(
                                onClick = {
                                    val pxmr = counterXmr.toDoubleOrNull()
                                        ?.let { (it * 1e12).toLong() }
                                    if (pxmr != null) {
                                        busy = true; error = null
                                        scope.launch {
                                            withContext(Dispatchers.IO) {
                                                runCatching {
                                                    org.ducatproject.ducat.Ceremony
                                                        .proposeRideSplit(context, idHex, pxmr)
                                                }
                                            }.onFailure { error = it.message }
                                            busy = false
                                            countering = false
                                        }
                                    }
                                },
                                enabled = !busy && counterXmr.isNotBlank(),
                            ) { Text(stringResource(R.string.bond_counter)) }
                        }
                    }
                }
                stage == "release_cosigned" ->
                    BondLine(spin = false, text = stringResource(R.string.bond_released))
                else -> return@Column
            }
            error?.let {
                Spacer(Modifier.height(4.dp))
                Text(it, color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.labelSmall)
            }
        }
    }
}

/** A plain note under a button — the consequence of pressing it. */
@Composable
private fun BondNote(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSecondaryContainer,
    )
}

@Composable
private fun BondLine(spin: Boolean, text: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        if (spin) {
            CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 2.dp)
        } else {
            Icon(
                Icons.Filled.Lock, null, Modifier.size(14.dp),
                tint = MaterialTheme.colorScheme.onSecondaryContainer,
            )
        }
        Spacer(Modifier.width(8.dp))
        Text(
            text,
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSecondaryContainer,
        )
    }
}

/**
 * Propose a reservation to this contact (§15.12's Airbnb/Turo shape): rent
 * and both deposits, stated up front — the escrow will name its whole
 * arithmetic in the ceremony frame, and the host's phone shows exactly what
 * accepting costs. Nothing is at risk until money moves: the guest funds
 * rent + their deposit, and the host's acceptance IS funding theirs.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ReserveSheet(contact: Contact, onDone: () -> Unit) {
    val context = androidx.compose.ui.platform.LocalContext.current
    var rent by remember { mutableStateOf("") }
    var myDep by remember { mutableStateOf("") }
    var hostDep by remember { mutableStateOf("") }
    // What kind of thing is being handed over decides how much each side
    // stakes: a room is not a car (see Stakes.kt for where the numbers come
    // from). Picking one fills both deposits, and either can still be typed
    // over — the suggestion is a starting point, not a rule.
    var deal by remember { mutableStateOf(org.ducatproject.ducat.Stakes.Deal.Stay) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    fun pxmr(s: String): Long? = s.toDoubleOrNull()?.let { (it * 1e12).toLong() }?.takeIf { it > 0 }

    androidx.compose.material3.ModalBottomSheet(onDismissRequest = onDone) {
        Column(Modifier.padding(horizontal = 20.dp).padding(bottom = 24.dp)) {
            Text(stringResource(R.string.res_title), style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(4.dp))
            Text(
                stringResource(R.string.res_note),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(12.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                listOf(
                    org.ducatproject.ducat.Stakes.Deal.Stay to R.string.res_kind_stay,
                    org.ducatproject.ducat.Stakes.Deal.Vehicle to R.string.res_kind_vehicle,
                ).forEach { (d, label) ->
                    FilterChip(
                        selected = deal == d,
                        onClick = {
                            deal = d
                            // Re-suggest from whatever rent has been typed.
                            pxmr(rent)?.let { r ->
                                val st = org.ducatproject.ducat.Stakes.stakeFor(d, r)
                                val text = "%.6f".format(java.util.Locale.US, st / 1e12)
                                myDep = text; hostDep = text
                            }
                        },
                        label = { Text(stringResource(label)) },
                    )
                    Spacer(Modifier.width(8.dp))
                }
                Text(
                    stringResource(R.string.res_kind_note, deal.percent),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.height(12.dp))
            OutlinedTextField(
                value = rent,
                onValueChange = { typed ->
                    rent = typed.filter { c -> c.isDigit() || c == '.' }
                    // Suggest both stakes from the rent as it is typed. Either
                    // field can still be edited; this only saves the person
                    // doing percentages in their head at a bus stop.
                    pxmr(rent)?.let { r ->
                        val st = org.ducatproject.ducat.Stakes.stakeFor(deal, r)
                        val text = "%.6f".format(java.util.Locale.US, st / 1e12)
                        myDep = text
                        hostDep = text
                    }
                },
                label = { Text(stringResource(R.string.res_rent)) },
                singleLine = true, modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(8.dp))
            OutlinedTextField(
                value = myDep, onValueChange = { myDep = it.filter { c -> c.isDigit() || c == '.' } },
                label = { Text(stringResource(R.string.res_my_deposit)) },
                singleLine = true, modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(8.dp))
            OutlinedTextField(
                value = hostDep, onValueChange = { hostDep = it.filter { c -> c.isDigit() || c == '.' } },
                label = { Text(stringResource(R.string.res_host_deposit)) },
                singleLine = true, modifier = Modifier.fillMaxWidth(),
            )
            error?.let {
                Spacer(Modifier.height(6.dp))
                Text(it, color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall)
            }
            Spacer(Modifier.height(12.dp))
            Button(
                onClick = {
                    val r = pxmr(rent); val g = pxmr(myDep); val h = pxmr(hostDep)
                    if (r == null) { error = context.getString(R.string.res_need_rent); return@Button }
                    busy = true; error = null
                    scope.launch {
                        withContext(Dispatchers.IO) {
                            runCatching {
                                val arbHex = org.ducatproject.ducat.ArbiterStore(context).hex()
                                    ?.takeIf { it != contact.personaHex }
                                val arb = arbHex?.let { hx ->
                                    org.ducatproject.ducat.ContactStore(context).all()
                                        .firstOrNull { it.personaHex == hx }
                                }
                                org.ducatproject.ducat.Ceremony.startReservation(
                                    context, contact, arb, r, g ?: 0L, h ?: 0L,
                                )
                            }
                        }.onSuccess {
                            busy = false; onDone()
                        }.onFailure { error = it.message; busy = false }
                    }
                },
                enabled = !busy && rent.isNotBlank(),
                modifier = Modifier.fillMaxWidth().height(48.dp),
            ) { Text(stringResource(R.string.res_send)) }
        }
    }
}
