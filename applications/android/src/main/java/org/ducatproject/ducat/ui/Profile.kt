package org.ducatproject.ducat.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
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
    BackHandler(onBack = onBack)
    val context = LocalContext.current
    val store = remember { ContactStore(context) }
    // Read back from the store on every change rather than holding the
    // snapshot this screen was opened with.
    //
    // It was a one-shot `mutableStateOf(contact)`, refreshed by hand in the
    // one place that happened to remember to — so **"Accept" on a held
    // payment address did its work and left the warning on screen**. The store
    // logged the change, the money would have gone to the new address, and the
    // red card still said a card wanted to change it, with no way to tell
    // whether the tap had landed. Anything arriving from the poller while the
    // screen is open was invisible the same way.
    val version by ContactStore.changes.collectAsState()
    val c = remember(version, contact.personaHex) {
        store.all().firstOrNull { it.personaHex == contact.personaHex } ?: contact
    }
    // Saveable, keyed by whose profile this is: the field starts at what
    // they are called now, and what is in it after that is an edit somebody
    // is part-way through — which a turn of the phone silently reverted to
    // the stored name, with the Save button greying out to match.
    var petname by rememberSaveable(contact.personaHex) {
        mutableStateOf(contact.petname.orEmpty())
    }
    var saved by remember { mutableStateOf(false) }

    Scaffold(
        topBar = {
            TopAppBar(
                colors = TopAppBarDefaults.topAppBarColors(
                containerColor = MaterialTheme.colorScheme.background,
            ),
                            title = { Text(c.displayName()) },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.profile_back)) }
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
                            .clickable {
                                copyText(context, value, context.getString(R.string.profile_copied))
                            }
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
                supportingText = {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(stringResource(R.string.profile_your_name_for_them_support), Modifier.weight(1f))
                        CharCounter(petname.length, 32)
                    }
                },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(8.dp))
            Button(
                onClick = {
                    // The name and nothing else, read-at-write: `c` is as
                    // fresh as the last recomposition, and writing it back
                    // whole carried whatever counters a poll had advanced
                    // since — the rewind setPetname exists to prevent. It
                    // bumps the store, and `c` is derived from that.
                    store.setPetname(c.personaHex, petname.trim().ifBlank { null })
                    saved = true
                },
                // Only when there is something to save. It was always live, so
                // a contact you had never renamed showed a filled primary
                // button inviting a tap that stored the blank you were already
                // storing. Clearing a name you *had* set still counts as a
                // change, which is why this compares against the stored value
                // rather than checking the field is non-empty.
                enabled = petname.trim() != (c.petname ?: ""),
            ) {
                Text(
                    if (saved) stringResource(R.string.profile_saved)
                    else stringResource(R.string.profile_save_name),
                )
            }

            // §15.12: the third key in every ride escrow this device builds.
            // A choice about *this* contact, made where they are looked at.
            Spacer(Modifier.height(20.dp))
            val arbiters = remember { org.ducatproject.ducat.ArbiterStore(context) }
            var isArbiter by remember(c.personaHex) {
                mutableStateOf(arbiters.hex() == c.personaHex)
            }
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(
                        stringResource(R.string.profile_arbiter),
                        style = MaterialTheme.typography.bodyLarge,
                    )
                    Text(
                        stringResource(R.string.profile_arbiter_note),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.outline,
                    )
                }
                Switch(
                    checked = isArbiter,
                    onCheckedChange = { on ->
                        isArbiter = on
                        arbiters.set(if (on) c.personaHex else null)
                    },
                )
            }

            Spacer(Modifier.height(24.dp))
            Text(stringResource(R.string.profile_checked), style = MaterialTheme.typography.titleMedium)
            Text(
                stringResource(R.string.profile_checked_note),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            Field(stringResource(R.string.profile_persona), c.personaHex)

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
                // What they call themselves, which is to say what they typed.
                c.assertedName?.let { isolate(it) }
                    ?: stringResource(R.string.profile_none_given),
            )
            Field(
                stringResource(R.string.profile_monero_address),
                c.theirAddress ?: stringResource(R.string.profile_not_shared),
            )
            // A card asked to move this and was not allowed to. The decision
            // belongs here, beside the address it would replace, and it is
            // deliberately not a one-tap yes: the honest version of this is a
            // contact who lost their phone, and the way to tell the two apart
            // is to ask them — which the copy says, because nothing on this
            // screen can tell you.
            c.pendingAddress?.let { held ->
                Spacer(Modifier.height(12.dp))
                Card(
                    colors = CardDefaults.cardColors(
                        containerColor = MaterialTheme.colorScheme.errorContainer,
                    ),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(Modifier.padding(14.dp)) {
                        Text(
                            stringResource(R.string.profile_payto_held_title),
                            style = MaterialTheme.typography.titleSmall,
                            color = MaterialTheme.colorScheme.onErrorContainer,
                        )
                        Spacer(Modifier.height(4.dp))
                        Text(
                            stringResource(
                                R.string.profile_payto_held_body,
                                isolate(c.displayName()),
                            ),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onErrorContainer,
                        )
                        Spacer(Modifier.height(8.dp))
                        Text(
                            held,
                            fontFamily = FontFamily.Monospace,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onErrorContainer,
                        )
                        Spacer(Modifier.height(8.dp))
                        Row {
                            TextButton(onClick = {
                                ContactStore(context).dismissPendingAddress(c.personaHex)
                            }) { Text(stringResource(R.string.profile_payto_held_keep)) }
                            Spacer(Modifier.width(8.dp))
                            TextButton(onClick = {
                                ContactStore(context).acceptPendingAddress(c.personaHex)
                            }) { Text(stringResource(R.string.profile_payto_held_accept)) }
                        }
                    }
                }
            }

            Spacer(Modifier.height(24.dp))
            Text(stringResource(R.string.profile_where_reached), style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(8.dp))
            Field(stringResource(R.string.profile_their_outbox), c.theirOutbox.ifBlank { "—" })
            Field(stringResource(R.string.profile_your_outbox), c.myOutbox.ifBlank { "—" })

            Spacer(Modifier.height(24.dp))
            Button(onClick = { onOpenChat(c) }, modifier = Modifier.fillMaxWidth()) {
                Text(stringResource(R.string.profile_open_chat))
            }

            Spacer(Modifier.height(24.dp))
            BondSection(c)

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
private fun BondSection(c: Contact) {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var choosingArbiter by remember { mutableStateOf(false) }
    // **The bond's four acts run off the screen.**
    //
    // Each is a ceremony round or a chain scan — seconds, sometimes tens of
    // them — and they ran on this section's own scope, inside a sheet over
    // a chat. Turning the phone, or a call arriving, cancelled the line
    // that shows what happened; the round itself finished. A deposit whose
    // return failed said nothing, and the button came back live for a
    // second attempt at something that may have been in flight.
    val bondKey = "bond:${c.personaHex}"
    val sendTick by ThreadSends.ticks.collectAsState()
    LaunchedEffect(sendTick, c.personaHex) {
        for (o in ThreadSends.take(bondKey)) when (o) {
            // A landing with something to say says it: the call-off that
            // found money still in the escrow answers with that sentence.
            is ThreadSends.Outcome.Landed -> error = o.result
            is ThreadSends.Outcome.Failed -> {
                // The screen speaks in sentences; the log keeps the
                // exception, or the sentence is all anyone ever learns.
                org.ducatproject.ducat.DucatLog.w(
                    "Profile",
                    "bond: ${o.error.javaClass.simpleName}: ${o.error.message}",
                )
                error = moneyFailure(context, o.error)
            }
        }
        busy = ThreadSends.inFlight(bondKey)
    }
    /** One of this section's acts, run where the screen cannot cancel it.
     *  Whatever it returns is shown as the answer. */
    val act: (() -> String?) -> Unit = { body ->
        busy = true
        error = null
        ThreadSends.launch(ContactStore(context), bondKey, null) { body() }
    }
    // produceState on IO, not remember: Ceremony.all decrypts the whole
    // ceremony store, and this ran on the main thread — keyed on `busy`, so
    // every button press paid for it twice.
    // Null until read is "not looked yet", not "no bond": with the initial
    // value standing for both, every opening of a bonded contact's profile
    // showed "Post a bond" — live — for the length of the decrypt, and a
    // tap inside that flash started a second ceremony under the first.
    var looked by remember { mutableStateOf(false) }
    val ceremony by produceState<org.json.JSONObject?>(null, version, busy) {
        value = withContext(Dispatchers.IO) {
            org.ducatproject.ducat.Ceremony.all(context)
                .filter {
                    it.optString("peer") == c.personaHex &&
                        // Bonds only. Unfiltered, this section wore a ride
                        // escrow's stage as the bond's: a stranded fare
                        // showed "Co-signed — theirs to broadcast" here,
                        // and the Return button then failed against a bond
                        // that never existed. A ride's state lives in its
                        // thread's banner, which has the machinery for it.
                        it.optInt("kind") == org.ducatproject.ducat.Ceremony.KIND_BOND &&
                        !org.ducatproject.ducat.Ceremony.isArbiter(it)
                }
                // Newest by its own clock — prefs iteration order is nobody's
                // promise, and lastOrNull() was betting on it.
                .maxByOrNull { it.optLong("created") }
        }
        looked = true
    }

    fun post(arbiter: org.ducatproject.ducat.Contact?) {
        choosingArbiter = false
        act {
            org.ducatproject.ducat.Ceremony.startBond(context, c, arbiter)
            null
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
                        ) { Text(stringResource(R.string.profile_bond_with_arbiter, isolate(a.displayName()))) }
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

    when (if (looked) ceremony?.optString("stage").orEmpty() else "looking") {
        "looking" -> {}
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
                    act {
                        org.ducatproject.ducat.Ceremony.releaseBond(context, c)
                        null
                    }
                },
            ) {
                if (busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                else Text(stringResource(R.string.profile_bond_release))
            }
            // A bond nobody ever paid into. "Return the deposit" is the only
            // way off this screen and it fails against an empty address, so a
            // built-and-abandoned bond sat here for good — and, being the
            // newest record with this contact, hid the button for the next
            // one. Ceremony.callOff is the ending, but its own guard reads
            // markers only a ride writes; a bond is funded by an ordinary
            // send to the address and records nothing, so the chain is asked
            // first, the way onAbort asks it, and an address that cannot be
            // read is not called empty.
            TextButton(
                enabled = !busy,
                contentPadding = PaddingValues(horizontal = 0.dp, vertical = 0.dp),
                onClick = {
                    val o = ceremony ?: return@TextButton
                    val idHex = o.optString("id")
                    act {
                        val keys = org.ducatproject.ducat.hexToBytes(o.optString("keys"))
                            ?: throw IllegalStateException("this device holds no key share")
                        val nodeUrl = org.ducatproject.ducat.NodeStore(context).lastGood()
                            ?: runCatching {
                                uniffi.ducat_mobile.moneroPickNode(
                                    uniffi.ducat_mobile.moneroDefaultNodes(null), "stagenet", 8000u,
                                ).url
                            }.getOrNull()
                            ?: throw org.ducatproject.ducat.Ceremony.NoNode()
                        val from = o.optLong("scanFrom").takeIf { it > 0 }
                            ?: org.ducatproject.ducat.WalletStore(context).restoreHeight().toLong()
                        val bal = uniffi.ducat_mobile.escrowBalance(keys, nodeUrl, from.toULong()).toLong()
                        if (bal == 0L) org.ducatproject.ducat.Ceremony.callOff(context, idHex)
                        // Money still in there is the answer, not a failure:
                        // it is why the bond cannot simply be dropped.
                        if (bal > 0) {
                            context.getString(
                                R.string.profile_bond_holds,
                                org.ducatproject.ducat.Amounts.show(context, bal).primary,
                            )
                        } else null
                    }
                },
            ) { Text(stringResource(R.string.profile_bond_call_off)) }
            error?.let {
                Spacer(Modifier.height(4.dp))
                // The sentence as it comes: "Could not start:" is the
                // opening branch's prefix, and here nothing was being
                // started — a return that failed read "Could not start:
                // the escrow holds 0.5 XMR — return the deposit instead".
                Text(
                    it,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
        // The half of the ceremony this section never had: a proposal
        // WAITING on this device. Without it, an arbiter asked to rule —
        // or a bond partner asked to countersign the deposit's return —
        // reached a screen showing only the escrow's address, and the
        // whole 2-of-3 recovery path was a corridor with no door at the
        // end: the share existed, frostCosign worked, nothing called it.
        "release_pending" -> {
            var pinAsk by remember { mutableStateOf(false) }
            val back = ceremony?.optLong("pendingRiderBack", -1L) ?: -1L
            Text(
                if (back >= 0) {
                    stringResource(
                        R.string.profile_sign_pending_amt,
                        org.ducatproject.ducat.Amounts.show(context, back).primary,
                    )
                } else {
                    stringResource(R.string.profile_sign_pending_all)
                },
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(8.dp))
            Button(
                enabled = !busy,
                onClick = { pinAsk = true },
            ) {
                if (busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                else Text(stringResource(R.string.profile_sign_release))
            }
            PinGate(
                open = pinAsk,
                onDismiss = { pinAsk = false },
                onPassed = {
                    pinAsk = false
                    // Read here, not inside the act: by the time that runs
                    // this screen may be gone and its ceremony with it.
                    val idHex = ceremony?.optString("id").orEmpty()
                    act {
                        org.ducatproject.ducat.Ceremony.approveRideRelease(context, idHex)
                        null
                    }
                },
            )
            error?.let {
                Spacer(Modifier.height(4.dp))
                Text(
                    it,
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
        // Called off, by either side. Like "released", an ending and not a
        // door: the fallthrough below showed the dead escrow's address with
        // no button, and the next bond had nowhere to start.
        "aborted" -> {
            Text(stringResource(R.string.profile_bond_called_off),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant)
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
        )
    }
}

@Composable
private fun Field(
    label: String,
    value: String,
) {
    val context = LocalContext.current
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
                TextButton(onClick = {
                    copyText(context, value, context.getString(R.string.profile_copied))
                }) {
                    Text(stringResource(R.string.profile_copy), style = MaterialTheme.typography.labelSmall)
                }
            }
        }
    }
}
