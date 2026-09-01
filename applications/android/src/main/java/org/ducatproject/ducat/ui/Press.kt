package org.ducatproject.ducat.ui

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog
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
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    val pubs = remember(version) { Publications.publications(context) }

    // No press yet: the one-field beginning, same as the Publishing room.
    if (pubs.isEmpty()) {
        var name by remember { mutableStateOf("") }
        Column(
            Modifier.fillMaxSize().padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                stringResource(R.string.press_none_yet),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(12.dp))
            OutlinedTextField(
                name, { name = it },
                label = { Text(stringResource(R.string.press_name_hint)) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(8.dp))
            Button(
                enabled = name.isNotBlank(),
                onClick = {
                    scope.launch(Dispatchers.IO) {
                        runCatching {
                            val id = Publications.create(context, name.trim())
                            Publications.setPressPub(context, id)
                        }
                        ContactStore.bump()
                    }
                },
            ) { Text(stringResource(R.string.press_create)) }
        }
        return
    }

    val pubId = Publications.pressPub(context) ?: pubs.first().first
    val pubName = pubs.firstOrNull { it.first == pubId }?.second ?: return
    var code by remember(pubId) { mutableStateOf<String?>(null) }
    LaunchedEffect(pubId, version) {
        code = withContext(Dispatchers.IO) {
            runCatching { Publications.standingCode(context, pubId) }.getOrNull()
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
    val scope = androidx.compose.runtime.rememberCoroutineScope()
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
    var busy by remember { mutableStateOf(false) }
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
                enabled = !busy && picked.isNotEmpty() && code != null,
                onClick = {
                    busy = true
                    scope.launch(Dispatchers.IO) {
                        val store = ContactStore(context)
                        for (hex in picked) {
                            // The freshest copy each time: send advances the
                            // contact, and a stale snapshot reuses a slot.
                            val c = store.all().firstOrNull { it.personaHex == hex }
                                ?: continue
                            runCatching {
                                Mailbox.send(
                                    context, c,
                                    context.getString(
                                        R.string.press_invite_body, pubName,
                                    ) + "\n" + code,
                                )
                            }.onFailure {
                                DucatLog.w("Press", "invite: ${it.message}")
                            }
                        }
                        withContext(Dispatchers.Main) { onDone() }
                    }
                },
            ) { Text(stringResource(R.string.press_invite_send)) }
        },
        dismissButton = {
            TextButton(onClick = onDone) { Text(stringResource(R.string.press_invite_later)) }
        },
    )
}
