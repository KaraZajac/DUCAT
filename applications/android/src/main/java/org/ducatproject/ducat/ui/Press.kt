package org.ducatproject.ducat.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.QrCode
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.Publications
import org.ducatproject.ducat.R

/**
 * The Press face (§15.11 stance, §16.20 machinery): the phone on the
 * zine table. One publication forward, its standing subscribe code big
 * enough to scan across a counter, and the two numbers a publisher
 * glances at — who subscribed, what the period costs. Inviting contacts
 * sends the same shared code down the threads they already have.
 */
@Composable
fun PressScreen() {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val pubs = remember(version) { Publications.publications(context) }

    // No press yet: the code stands for a publication, so this screen has
    // nothing to show — it says where publications come from and takes you
    // there. Creation lives in one place, the Press room.
    if (pubs.isEmpty()) {
        Column(
            Modifier.fillMaxSize().padding(32.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Box(
                Modifier.size(72.dp)
                    .clip(CircleShape)
                    .background(MaterialTheme.colorScheme.surfaceVariant),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    Icons.Filled.QrCode,
                    contentDescription = null,
                    modifier = Modifier.size(36.dp),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.height(16.dp))
            Text(
                stringResource(R.string.press_no_code_title),
                style = MaterialTheme.typography.titleLarge,
            )
            Spacer(Modifier.height(8.dp))
            Text(
                stringResource(R.string.press_no_code_body),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )
            Spacer(Modifier.height(20.dp))
            Button(onClick = { shellTabRequest.value = 1 }) {
                Text(stringResource(R.string.press_open_room))
            }
        }
        return
    }

    val pubId = Publications.pressPub(context) ?: pubs.first().first
    val pubName = pubs.firstOrNull { it.first == pubId }?.second ?: return
    var code by remember(pubId) { mutableStateOf<String?>(null) }
    // The code on record, or a mint off the screen. This called
    // `standingCode` — read-or-mint — from the screen's own effect, keyed on
    // every contact-store change: a bump while the mint was out (a poll
    // delivering, a rotation rebuilding the screen) started a second mint
    // beside the first, and each bound another card to the publication
    // before the record said one existed. The mint runs once, under a key
    // the rebuilt screen checks before starting its own; a failed mint
    // waits longer each time rather than spinning against a node that is
    // not attached yet.
    val kPress = "press:$pubId"
    val tick by ThreadSends.ticks.collectAsState()
    var wait by remember(pubId) { mutableStateOf(0L) }
    LaunchedEffect(pubId, version, tick) {
        val failed = ThreadSends.take(kPress).any {
            it !is ThreadSends.Outcome.Landed || it.result == null
        }
        if (failed) wait = (wait * 2).coerceIn(5_000L, 60_000L)
        val standing = withContext(Dispatchers.IO) { Publications.pressCode(context, pubId) }
        if (standing != null) { code = standing; wait = 0L; return@LaunchedEffect }
        if (ThreadSends.inFlight(kPress)) return@LaunchedEffect
        if (wait > 0L) delay(wait)
        ThreadSends.launch(ContactStore(context), kPress, null) {
            Publications.standingCode(context, pubId)
        }
    }
    val price = remember(version, pubId) { Publications.priceOf(context, pubId) }
    val subs = remember(version, pubId) {
        runCatching { Publications.subscribers(context, pubId).size }.getOrDefault(0)
    }
    var inviting by remember { mutableStateOf(false) }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        if (pubs.size > 1) {
            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                pubs.forEach { (id, n) ->
                    FilterChip(
                        selected = id == pubId,
                        onClick = { Publications.setPressPub(context, id) },
                        label = { Text(n) },
                    )
                }
            }
            Spacer(Modifier.height(8.dp))
        }
        Text(pubName, style = MaterialTheme.typography.headlineSmall)
        Spacer(Modifier.height(2.dp))
        Text(
            if (price > 0) {
                stringResource(R.string.market_per_period, Amounts.show(context, price).primary)
            } else {
                stringResource(R.string.market_free)
            },
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.ducat.settled,
        )
        Spacer(Modifier.height(16.dp))
        code?.let { QrBlock(it) } ?: Text(
            stringResource(R.string.press_code_minting),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(8.dp))
        Text(
            stringResource(R.string.press_scan),
            style = MaterialTheme.typography.titleMedium,
        )
        Spacer(Modifier.height(4.dp))
        Text(
            stringResource(R.string.press_subscribed, subs),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(16.dp))
        OutlinedButton(onClick = { inviting = true }) {
            Text(stringResource(R.string.press_invite))
        }
    }

    if (inviting) {
        InviteSheet(pubName = pubName, code = code, onDone = { inviting = false })
    }
}

/** Pick contacts; each gets the shared code as a tappable line in their
 *  thread. Claiming it is subscribing — consent stays with the reader. */
@Composable
private fun InviteSheet(pubName: String, code: String?, onDone: () -> Unit) {
    val context = LocalContext.current
    val personas = remember { PersonaStore(context) }
    val contacts = remember {
        val all = ContactStore(context).all()
        if (personas.all().size > 1) {
            val worn = personas.worn()
            all.filter { personas.ownerHexOf(it) == worn }
        } else {
            all
        }
    }
    var picked by remember { mutableStateOf(setOf<String>()) }
    AlertDialog(
        onDismissRequest = onDone,
        title = { Text(stringResource(R.string.press_invite)) },
        text = {
            if (contacts.isEmpty()) {
                Text(stringResource(R.string.press_invite_nobody))
            } else {
                LazyColumn(Modifier.height(320.dp)) {
                    items(contacts.size) { i ->
                        val c = contacts[i]
                        Row(
                            Modifier.fillMaxWidth(),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Checkbox(
                                checked = c.personaHex in picked,
                                onCheckedChange = {
                                    picked = if (it) picked + c.personaHex
                                    else picked - c.personaHex
                                },
                            )
                            Text(c.displayName())
                        }
                    }
                }
            }
        },
        confirmButton = {
            Button(
                enabled = picked.isNotEmpty() && code != null,
                onClick = {
                    // One send per thread, off the sheet. These ran in the
                    // sheet's own scope, dispatched to IO: a rotation or a
                    // call in the moment before a worker picked the job up
                    // cancelled it unstarted, and nobody was invited — with
                    // the sheet gone, nothing said so either. Each thread's
                    // registry job runs whatever happens to this sheet, and a
                    // failure is heard by the thread it belongs to, in the
                    // words that screen uses for a send that did not go.
                    val store = ContactStore(context)
                    val what = context.getString(R.string.chat_what_invite)
                    val text = context.getString(R.string.press_invite_body, pubName) + "\n" + code
                    for (hex in picked) {
                        ThreadSends.launch(store, hex, what) {
                            // The freshest copy each time: send advances the
                            // contact, and a stale snapshot reuses a slot.
                            val c = store.all().firstOrNull { it.personaHex == hex }
                                ?: return@launch null
                            Mailbox.send(context, c, text)
                            null
                        }
                    }
                    onDone()
                },
            ) { Text(stringResource(R.string.press_invite_send)) }
        },
        dismissButton = {
            TextButton(onClick = onDone) { Text(stringResource(R.string.press_invite_later)) }
        },
    )
}
