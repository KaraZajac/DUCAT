package org.ducatproject.ducat.ui

import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.NameStore
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.R
import uniffi.ducat_mobile.createContactCard
import uniffi.ducat_mobile.readContactCard

/**
 * Contacts, and the one screen where DUCAT stops being anonymous on purpose.
 *
 * §16.3 keeps transactions anonymous — a stall never learns who paid. §16.9 is
 * the other relationship: people you already know, where the point is that you
 * *do* know. Both are in the app because both are real, and the UI has to make
 * which one you are in obvious rather than blurring them together.
 */
@Composable
private fun ContactRow(c: Contact, onClick: () -> Unit) {
    ListItem(
        headlineContent = { Text(c.displayName()) },
        supportingContent = {
            Text(
                // §16.9: the asserted name is worth what the channel was worth.
                // Showing the key fragment gives a user something to compare out
                // of band, which is the only check available for a card that
                // arrived through someone else's app.
                c.personaHex.take(16) + "…",
                fontFamily = FontFamily.Monospace,
                style = MaterialTheme.typography.bodySmall,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        },
        leadingContent = {
            Box(
                Modifier
                    .size(40.dp)
                    .background(MaterialTheme.colorScheme.secondaryContainer, RoundedCornerShape(20.dp)),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    c.displayName().take(1).uppercase(),
                    color = MaterialTheme.colorScheme.onSecondaryContainer,
                    fontWeight = FontWeight.Bold,
                )
            }
        },
        trailingContent = { Icon(Icons.Filled.ChatBubbleOutline, stringResource(R.string.contacts_chat)) },
        modifier = Modifier.clickable { onClick() },
    )
}

/**
 * Handing your card over.
 *
 * The URI is the primary form because it is the channel people actually use —
 * it pastes into any messenger. §16.9 measured the card at about 1 KB, which
 * makes a QR dense but valid and a URI unconstrained.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ShareCardSheet(personaSecret: ByteArray?, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current
    val scope = rememberCoroutineScope()
    var name by remember { mutableStateOf(NameStore(context).get() ?: "") }
    var uri by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(Modifier.padding(20.dp).verticalScroll(rememberScrollState())) {
            Text(stringResource(R.string.contacts_share_card_title), style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(8.dp))
            Text(
                stringResource(R.string.contacts_share_card_body),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(16.dp))
            OutlinedTextField(
                value = name,
                onValueChange = { if (it.length <= 32) name = it },
                label = { Text(stringResource(R.string.contacts_name_to_show_label)) },
                supportingText = { Text(stringResource(R.string.contacts_name_to_show_support)) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(16.dp))

            if (uri == null) {
                Button(
                    onClick = {
                        busy = true
                        error = null
                        scope.launch {
                            val r = withContext(Dispatchers.IO) {
                                runCatching {
                                    // Records, not a route: the card names an
                                    // inbox that outlives this process (§16.12).
                                    Mailbox.issueCard(
                                        context,
                                        name.ifBlank { null },
                                        60uL * 60uL * 24uL,
                                    )
                                }
                            }
                            busy = false
                            r.onSuccess {
                                NameStore(context).put(name)
                                uri = it.uri
                            }.onFailure { error = it.message ?: context.getString(R.string.contacts_error_make_card) }
                        }
                    },
                    enabled = !busy && personaSecret != null,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    if (busy) {
                        CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                        Spacer(Modifier.width(10.dp))
                        Text(stringResource(R.string.contacts_publishing_inbox))
                    } else {
                        Text(stringResource(R.string.contacts_create_card))
                    }
                }
                if (busy) {
                    Spacer(Modifier.height(8.dp))
                    Text(
                        stringResource(R.string.contacts_publishing_note),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            } else {
                QrBlock(uri!!)
                Spacer(Modifier.height(16.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Button(
                        onClick = { clipboard.setText(AnnotatedString(uri!!)) },
                        modifier = Modifier.weight(1f),
                    ) {
                        Icon(Icons.Filled.ContentCopy, null, Modifier.size(18.dp))
                        Spacer(Modifier.width(8.dp))
                        Text(stringResource(R.string.contacts_copy_link))
                    }
                    OutlinedButton(
                        onClick = { shareText(context, uri!!) },
                        modifier = Modifier.weight(1f),
                    ) {
                        Icon(Icons.Filled.Send, null, Modifier.size(18.dp))
                        Spacer(Modifier.width(8.dp))
                        Text(stringResource(R.string.contacts_send))
                    }
                }
                Spacer(Modifier.height(12.dp))
                Text(
                    stringResource(R.string.contacts_expires_24h),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            error?.let {
                Spacer(Modifier.height(12.dp))
                Text(it, color = MaterialTheme.colorScheme.error)
            }
            Spacer(Modifier.height(24.dp))
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun AddContactSheet(onDismiss: () -> Unit, onAdded: () -> Unit, store: ContactStore) {
    val clipboard = LocalClipboardManager.current
    var text by remember { mutableStateOf("") }
    var scanned by remember { mutableStateOf<uniffi.ducat_mobile.ScannedCard?>(null) }
    var petname by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var adding by remember { mutableStateOf(false) }
    var scanning by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val context = LocalContext.current

    if (scanning) {
        QrScanner(
            prompt = stringResource(R.string.contacts_scan_prompt),
            onResult = { raw ->
                scanning = false
                // Read it here rather than dropping the text into the box: a
                // scan that only fills a field makes the user press a button to
                // find out it was the wrong code.
                runCatching { readContactCard(raw) }
                    .onSuccess {
                        if (it.expired) error = context.getString(R.string.contacts_card_expired_ask_new)
                        else { scanned = it; petname = it.assertedName ?: "" }
                    }
                    .onFailure { error = context.getString(R.string.contacts_not_a_card); text = raw }
            },
            onDismiss = { scanning = false },
        )
        return
    }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(Modifier.padding(20.dp).verticalScroll(rememberScrollState())) {
            Text(stringResource(R.string.contacts_add_contact_title), style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(16.dp))

            if (scanned == null) {
                OutlinedTextField(
                    value = text,
                    onValueChange = { text = it; error = null },
                    label = { Text(stringResource(R.string.contacts_paste_link_label)) },
                    modifier = Modifier.fillMaxWidth(),
                    minLines = 3,
                )
                Spacer(Modifier.height(8.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    TextButton(onClick = {
                        clipboard.getText()?.let { text = it.text }
                    }) { Text(stringResource(R.string.contacts_paste)) }
                    TextButton(onClick = { scanning = true }) {
                        Icon(Icons.Filled.QrCodeScanner, null, Modifier.size(18.dp))
                        Spacer(Modifier.width(6.dp))
                        Text(stringResource(R.string.contacts_scan))
                    }
                }
                Spacer(Modifier.height(8.dp))
                Button(
                    onClick = {
                        runCatching { readContactCard(text) }
                            .onSuccess {
                                if (it.expired) {
                                    error = context.getString(R.string.contacts_card_expired_ask_them)
                                } else {
                                    scanned = it
                                    petname = it.assertedName ?: ""
                                }
                            }
                            .onFailure { error = it.message ?: context.getString(R.string.contacts_not_a_card) }
                    },
                    enabled = text.isNotBlank(),
                    modifier = Modifier.fillMaxWidth(),
                ) { Text(stringResource(R.string.contacts_read_card)) }
            } else {
                val s = scanned!!
                // §16.9 requires the asserted name be shown as unverified. A
                // card that arrived through a messaging app was authenticated by
                // *that app*, and the UI must not launder that into a claim
                // DUCAT is making.
                Card(
                    colors = CardDefaults.cardColors(
                        containerColor = MaterialTheme.colorScheme.surfaceVariant
                    )
                ) {
                    Column(Modifier.padding(16.dp)) {
                        Text(
                            s.assertedName ?: stringResource(R.string.contacts_no_name_given),
                            style = MaterialTheme.typography.titleMedium,
                        )
                        Spacer(Modifier.height(4.dp))
                        Text(
                            stringResource(R.string.contacts_unverified_name),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.error,
                        )
                        Spacer(Modifier.height(10.dp))
                        Text(
                            s.persona.toHex(),
                            fontFamily = FontFamily.Monospace,
                            style = MaterialTheme.typography.bodySmall,
                        )
                        Spacer(Modifier.height(6.dp))
                        Text(
                            stringResource(R.string.contacts_check_characters),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                Spacer(Modifier.height(16.dp))
                OutlinedTextField(
                    value = petname,
                    onValueChange = { if (it.length <= 32) petname = it },
                    label = { Text(stringResource(R.string.contacts_save_them_as_label)) },
                    supportingText = { Text(stringResource(R.string.contacts_save_them_as_support)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(16.dp))
                Button(
                    onClick = {
                        adding = true
                        error = null
                        scope.launch {
                            val r = withContext(Dispatchers.IO) {
                                // §16.3's rule holds for cards too: a contact is
                                // mutual or it is not one. Claiming publishes
                                // *our* details in the reply subkey, which is
                                // also what tells the issuer their card is spent.
                                runCatching {
                                    Mailbox.claimCard(context, s, petname.ifBlank { null })
                                }
                            }
                            adding = false
                            r.onSuccess { onAdded() }
                                .onFailure { error = context.getString(claimFailureRes(it)) }
                        }
                    },
                    enabled = petname.isNotBlank() && !adding,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    if (adding) {
                        CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                        Spacer(Modifier.width(10.dp))
                        Text(stringResource(R.string.contacts_reading_inbox))
                    } else {
                        Text(stringResource(R.string.contacts_add_contact_button))
                    }
                }
            }

            error?.let {
                Spacer(Modifier.height(12.dp))
                Text(it, color = MaterialTheme.colorScheme.error)
            }
            Spacer(Modifier.height(24.dp))
        }
    }
}

/**
 * Turn a refusal into something a person can act on.
 *
 * The wire codes are §18.5's and are for implementations. "Replay" is a correct
 * answer and a useless sentence; what the user needs to know is that the card
 * was already used and they should ask for another.
 */
private fun replyReason(context: Context, raw: String): String = when {
    raw.contains("Replay") -> context.getString(R.string.contacts_reply_replay)
    raw.contains("Expired") -> context.getString(R.string.contacts_reply_expired)
    raw.contains("BadSig") -> context.getString(R.string.contacts_reply_badsig)
    raw.contains("PolicyRefused") -> context.getString(R.string.contacts_reply_own_card)
    else -> context.getString(R.string.contacts_reply_refused)
}

private fun shareText(context: Context, text: String) {
    val i = android.content.Intent(android.content.Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(android.content.Intent.EXTRA_TEXT, text)
    }
    context.startActivity(android.content.Intent.createChooser(i, context.getString(R.string.contacts_send_your_card)))
}

fun ByteArray.toHex(): String = joinToString("") { "%02x".format(it) }
