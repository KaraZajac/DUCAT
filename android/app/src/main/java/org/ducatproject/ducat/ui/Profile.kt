package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.R
import androidx.compose.foundation.clickable

/**
 * Who this contact is, as far as anything can actually be known.
 *
 * The distinction the screen has to carry is between what was *checked* and
 * what was merely *said*. A petname is the user's own, so it is reliable by
 * construction. A persona key is cryptographic and every card is verified
 * against it. An asserted name and an address are neither — they arrived from
 * the other side and nothing here proves they belong to whoever handed the card
 * over.
 *
 * §16.9 makes that split the whole point of the contact model, so the screen
 * groups by it rather than by field type.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ContactProfile(contact: Contact, onBack: () -> Unit, onOpenChat: (Contact) -> Unit) {
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current
    val store = remember { ContactStore(context) }
    var c by remember { mutableStateOf(contact) }
    var petname by remember { mutableStateOf(contact.petname.orEmpty()) }
    var saved by remember { mutableStateOf(false) }

    Scaffold(
        topBar = {
            TopAppBar(
                colors = TopAppBarDefaults.topAppBarColors(
                containerColor = MaterialTheme.colorScheme.background,
            ),
                            title = { Text(c.displayName()) },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.Filled.ArrowBack, stringResource(R.string.profile_back)) }
                },
            )
        },
    ) { padding ->
        Column(
            Modifier
                .padding(padding)
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(20.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Avatar(c.displayName(), c.avatar, size = 64)
                Spacer(Modifier.width(14.dp))
                Column {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(c.displayName(), style = MaterialTheme.typography.headlineSmall)
                        c.pronouns?.let { code ->
                            // The wire carries the code (§16.9); the label is
                            // presentation and follows the app language.
                            val labels =
                                androidx.compose.ui.res.stringArrayResource(R.array.pronoun_labels)
                            labels.getOrNull(code - 1)?.let {
                                Spacer(Modifier.width(8.dp))
                                Text(
                                    it,
                                    style = MaterialTheme.typography.labelMedium,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                        }
                    }
                    c.assertedName?.takeIf { it != c.petname }?.let {
                        Text(
                            stringResource(R.string.profile_calls_themselves, it),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }

            // Their claim about themselves, and labelled as one. Nothing here
            // is verified by anything: DUCAT binds a persona to a key, and
            // binds that key to nothing in the outside world. An email shown
            // beside a persona is what that persona said, which is useful and
            // is not identity.
            val told = listOfNotNull(
                c.email?.let { stringResource(R.string.profile_label_email) to it },
                c.phone?.let { stringResource(R.string.profile_label_phone) to it },
                c.signal?.let { stringResource(R.string.profile_label_signal) to it },
            )
            if (told.isNotEmpty()) {
                Spacer(Modifier.height(18.dp))
                Text(stringResource(R.string.profile_what_they_shared), style = MaterialTheme.typography.titleSmall)
                Spacer(Modifier.height(2.dp))
                Text(
                    stringResource(R.string.profile_their_claim_note),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.outline,
                )
                Spacer(Modifier.height(8.dp))
                told.forEach { (label, value) ->
                    Row(
                        Modifier.fillMaxWidth()
                            .clickable { clipboard.setText(AnnotatedString(value)) }
                            .padding(vertical = 6.dp),
                    ) {
                        Text(
                            label,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.width(72.dp),
                        )
                        Text(value, style = MaterialTheme.typography.bodySmall)
                    }
                }
            }

            Spacer(Modifier.height(20.dp))
            OutlinedTextField(
                value = petname,
                onValueChange = { if (it.length <= 32) { petname = it; saved = false } },
                label = { Text(stringResource(R.string.profile_your_name_for_them_label)) },
                supportingText = { Text(stringResource(R.string.profile_your_name_for_them_support)) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(8.dp))
            Button(onClick = {
                store.add(c.copy(petname = petname.trim().ifBlank { null }))
                c = store.all().first { it.personaHex == c.personaHex }
                saved = true
            }) { Text(if (saved) stringResource(R.string.profile_saved) else stringResource(R.string.profile_save_name)) }

            Spacer(Modifier.height(24.dp))
            Text(stringResource(R.string.profile_checked), style = MaterialTheme.typography.titleMedium)
            Text(
                stringResource(R.string.profile_checked_note),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            Field(stringResource(R.string.profile_persona), c.personaHex, clipboard)

            Spacer(Modifier.height(24.dp))
            Text(stringResource(R.string.profile_told_to_you), style = MaterialTheme.typography.titleMedium)
            Text(
                stringResource(R.string.profile_told_note),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.ducat.changePending,
            )
            Spacer(Modifier.height(8.dp))
            Field(
                stringResource(R.string.profile_their_name_label),
                c.assertedName ?: stringResource(R.string.profile_none_given),
                clipboard,
            )
            Field(
                stringResource(R.string.profile_monero_address),
                c.theirAddress ?: stringResource(R.string.profile_not_shared),
                clipboard,
            )

            Spacer(Modifier.height(24.dp))
            Text(stringResource(R.string.profile_where_reached), style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(8.dp))
            Field(stringResource(R.string.profile_their_outbox), c.theirOutbox.ifBlank { "—" }, clipboard)
            Field(stringResource(R.string.profile_your_outbox), c.myOutbox.ifBlank { "—" }, clipboard)

            Spacer(Modifier.height(24.dp))
            Button(onClick = { onOpenChat(c) }, modifier = Modifier.fillMaxWidth()) {
                Text(stringResource(R.string.profile_open_chat))
            }

            Spacer(Modifier.height(24.dp))
            BondSection(c, clipboard)

            Spacer(Modifier.height(28.dp))
            // Named rather than silently absent: a profile screen with no
            // mention of these reads as "DUCAT has no notion of them", when the
            // truth is they are next.
            Text(stringResource(R.string.profile_not_built_yet), style = MaterialTheme.typography.titleSmall)
            Spacer(Modifier.height(4.dp))
            Text(
                stringResource(R.string.profile_not_built_note),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/**
 * The bond with this contact (§17.9): none yet, in ceremony, or done.
 *
 * One button starts the DKG; every later round arrives through the poll loop
 * and advances the engine without this screen's help, so all the section does
 * is read the recorded stage back — keyed on the store version, because a
 * ceremony only ever advances when a message lands, and message arrival is
 * exactly what bumps it.
 */
@Composable
private fun BondSection(
    c: Contact,
    clipboard: androidx.compose.ui.platform.ClipboardManager,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val version by ContactStore.changes.collectAsState()
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var choosingArbiter by remember { mutableStateOf(false) }
    val ceremony = remember(version, busy) {
        org.ducatproject.ducat.Ceremony.all(context)
            .filter { it.optString("peer") == c.personaHex }
            .lastOrNull()
    }

    fun post(arbiter: org.ducatproject.ducat.Contact?) {
        busy = true; error = null; choosingArbiter = false
        scope.launch {
            val r = withContext(Dispatchers.IO) {
                runCatching {
                    org.ducatproject.ducat.Ceremony.startBond(context, c, arbiter)
                }
            }
            r.onFailure { error = it.message ?: "?" }
            busy = false
        }
    }

    Text(stringResource(R.string.profile_bond_title),
        style = MaterialTheme.typography.titleMedium)
    Text(
        stringResource(R.string.profile_bond_note),
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    Spacer(Modifier.height(8.dp))

    if (choosingArbiter) {
        // Everyone here must be a mutual contact of both sides — the shares
        // travel the pairwise threads, so a missing thread is a missing wire.
        val others = remember(version) {
            ContactStore(context).all().filter { it.personaHex != c.personaHex }
        }
        AlertDialog(
            onDismissRequest = { choosingArbiter = false },
            title = { Text(stringResource(R.string.profile_bond_arbiter_q)) },
            text = {
                Column {
                    Text(
                        stringResource(R.string.profile_bond_arbiter_note),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(Modifier.height(8.dp))
                    Row(
                        Modifier.fillMaxWidth().clickable { post(null) }
                            .padding(vertical = 10.dp),
                    ) { Text(stringResource(R.string.profile_bond_just_us)) }
                    others.forEach { a ->
                        Row(
                            Modifier.fillMaxWidth().clickable { post(a) }
                                .padding(vertical = 10.dp),
                        ) { Text(stringResource(R.string.profile_bond_with_arbiter, a.displayName())) }
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { choosingArbiter = false }) {
                    Text(stringResource(R.string.common_cancel))
                }
            },
        )
    }

    when (ceremony?.optString("stage").orEmpty()) {
        "" -> {
            // Sealing and sending the commitment is network work; the button
            // shows it working rather than freezing the profile.
            Button(
                enabled = !busy,
                onClick = {
                    val hasOthers = ContactStore(context).all()
                        .any { it.personaHex != c.personaHex }
                    if (hasOthers) choosingArbiter = true else post(null)
                },
            ) {
                if (busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                else Text(stringResource(R.string.profile_bond_post))
            }
            error?.let {
                Spacer(Modifier.height(4.dp))
                Text(
                    stringResource(R.string.profile_bond_failed, it),
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
        "committed" -> Text(stringResource(R.string.profile_bond_waiting),
            style = MaterialTheme.typography.bodyMedium)
        "shared" -> Text(stringResource(R.string.profile_bond_finishing),
            style = MaterialTheme.typography.bodyMedium)
        "done" -> {
            Field(
                stringResource(R.string.profile_bond_done),
                ceremony?.optString("address").orEmpty(),
                clipboard,
            )
            // Name the third keyholder when there is one: a 2-of-3 bond
            // behaves differently (nothing strands) and the screen should
            // say who makes that true.
            val arbIdx = ceremony?.optInt("arbiterIdx") ?: 0
            if (arbIdx > 0) {
                val arbHex = ceremony?.optJSONArray("roster")?.optString(arbIdx - 1)
                val arbName = remember(arbHex) {
                    ContactStore(context).all()
                        .firstOrNull { it.personaHex == arbHex }?.displayName()
                        ?: arbHex?.take(8)?.plus("…") ?: "?"
                }
                Text(
                    stringResource(R.string.profile_bond_with_arbiter, arbName),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.height(8.dp))
            // The other half of the ceremony: spend it back out. The deposit
            // returns to THIS device's wallet; the peer's co-signature is what
            // makes that possible at all, which is the point of a bond.
            Button(
                enabled = !busy,
                onClick = {
                    busy = true; error = null
                    scope.launch {
                        val r = withContext(Dispatchers.IO) {
                            runCatching {
                                org.ducatproject.ducat.Ceremony.releaseBond(context, c)
                            }
                        }
                        r.onFailure { error = it.message ?: "?" }
                        busy = false
                    }
                },
            ) {
                if (busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                else Text(stringResource(R.string.profile_bond_release))
            }
            error?.let {
                Spacer(Modifier.height(4.dp))
                Text(
                    stringResource(R.string.profile_bond_failed, it),
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
        "releasing" -> Text(stringResource(R.string.profile_bond_releasing),
            style = MaterialTheme.typography.bodyMedium)
        "release_cosigned" -> Text(stringResource(R.string.profile_bond_cosigned),
            style = MaterialTheme.typography.bodyMedium)
        "released" -> {
            Text(stringResource(R.string.profile_bond_released),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.ducat.settled)
            Spacer(Modifier.height(6.dp))
            Field(
                stringResource(R.string.profile_bond_txid),
                ceremony?.optString("txid").orEmpty(),
                clipboard,
            )
            // A returned deposit is a finished story, not a closed door —
            // the next bond starts from right here.
            Spacer(Modifier.height(8.dp))
            Button(
                enabled = !busy,
                onClick = {
                    val hasOthers = ContactStore(context).all()
                        .any { it.personaHex != c.personaHex }
                    if (hasOthers) choosingArbiter = true else post(null)
                },
            ) {
                if (busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                else Text(stringResource(R.string.profile_bond_post))
            }
            error?.let {
                Spacer(Modifier.height(4.dp))
                Text(
                    stringResource(R.string.profile_bond_failed, it),
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
        else -> Field(
            stringResource(R.string.profile_bond_done),
            ceremony?.optString("address").orEmpty(),
            clipboard,
        )
    }
}

@Composable
private fun Field(
    label: String,
    value: String,
    clipboard: androidx.compose.ui.platform.ClipboardManager,
) {
    Column(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
        Text(
            label,
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Row(verticalAlignment = Alignment.CenterVertically) {
            SelectionContainer(Modifier.weight(1f)) {
                Text(
                    value,
                    fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            if (!value.startsWith("—")) {
                TextButton(onClick = { clipboard.setText(AnnotatedString(value)) }) {
                    Text(stringResource(R.string.profile_copy), style = MaterialTheme.typography.labelSmall)
                }
            }
        }
    }
}
