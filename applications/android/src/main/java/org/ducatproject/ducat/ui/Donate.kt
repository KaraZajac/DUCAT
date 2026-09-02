package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.R
import org.ducatproject.ducat.WalletStore

/**
 * The donation box (§15.11, which defers to §15.9) — two stances, honestly
 * divided by what stands behind the code.
 *
 * The **static** target is `TapStatic` and stays deliberately not a contact
 * card: a sticker has no phone behind it, a claim-once card would die at its
 * first scan, and §15.9's limits apply in full — the address is reused (every
 * donor linkable to every other on the public ledger) and a swapped sticker
 * verifies. What it buys is the property print needs: any Monero wallet on
 * earth can pay it, no DUCAT on the donor's phone.
 *
 * The **live screen** is a phone, which is the thing §15.9's rule actually
 * asks for — so it may additionally offer a claim-once card with purpose
 * `donate`, recut the moment it is claimed, exactly as the bar tab and the
 * kiosk cut theirs. That one establishes the relationship on purpose: the
 * donation lands in a thread, and §15.11's vendor rule sends the receipt
 * back automatically — the donor's tax record, which a bare transfer can
 * never give them.
 */
@Composable
fun DonateScreen() {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    // Keyed, like `since` below already is. The version was being collected
    // three lines up and then not used for the three values that actually
    // change: somebody who set a name or a picture and came back to their
    // donation box found it still saying whatever it said before.
    val address = remember(version) { WalletStore(context).address() }
    val name = remember(version) { MyProfile(context).name() }
    val pic = remember(version) { MyProfile(context).avatar() }

    // What has arrived since this screen was opened — the busker's glance.
    // Counted by key image so a rescan cannot inflate it, and measured from
    // screen-open because "tonight" is the question a donation box answers.
    //
    // Saveable, because a rotation or a dark-mode switch recreates the
    // activity: an ordinary `remember` re-snapshotted the baseline against
    // whatever had arrived by then, so turning the phone sideways set the
    // evening's tally back to nothing.
    //
    // Both identifiers, not just the key image. An output that landed just
    // before the box was opened has not always derived its key image yet, and
    // recording only that let it turn up later looking like a new donation.
    // Which rail is showing. DUCAT first: the receipt is the reason this
    // screen grew a second code, and the Monero one is a tap away.
    var rail by rememberSaveable { mutableStateOf(0) }
    // The claim-once card, recut on claim — the bar's OpenTab pattern with
    // no tab at the end of it. Each claim opens a thread; the reconciler
    // (Donations) sends the receipt when the money is seen.
    var cardUri by remember { mutableStateOf<String?>(null) }
    var cardInbox by remember { mutableStateOf<String?>(null) }
    var cardError by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(rail, cardUri) {
        if (rail != 0 || cardUri != null) return@LaunchedEffect
        // Keeps trying. The box is stood up once and left — on a table,
        // in a case — and its first cut is often asked for before the node
        // has attached, which used to leave the offline sentence up for
        // the evening; the only way to a code was to switch rails and
        // back.
        val card = issueCardPatiently(context, 60uL * 60uL * 12uL, "donate") { cardError = it }
        // Cleared, or a recut after a bad evening showed the old
        // sentence in place of the spinner while the next cut ran.
        cardError = null
        cardUri = card.uri; cardInbox = card.inboxKey
    }
    LaunchedEffect(cardInbox) {
        val inbox = cardInbox ?: return@LaunchedEffect
        // The boot clock, because this measures a duration: a box standing
        // through a wall-clock change must not think its card younger or
        // older than it is.
        val cutAt = android.os.SystemClock.elapsedRealtime()
        while (true) {
            kotlinx.coroutines.delay(2_000)
            // A card burns two ways and both must recut it. Claimed is the
            // ordinary one. Expired is the donation box's own: the card is
            // cut for twelve hours and this is the one mode whose screen
            // plausibly stands longer than that — without this, a box left
            // up overnight showed a dead code and did not know. Recut an
            // hour early, so no donor ever scans the last minutes of one.
            if (android.os.SystemClock.elapsedRealtime() - cutAt > CARD_RECUT_MS) {
                cardUri = null; cardInbox = null
                break
            }
            val claimed = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                runCatching {
                    org.ducatproject.ducat.Mailbox.collectClaims(context)
                    ContactStore(context).claimantOf(inbox) != null
                }.getOrDefault(false)
            }
            if (claimed) {
                // Spent — cut the next donor's code.
                cardUri = null; cardInbox = null
                break
            }
        }
    }

    val atOpen = rememberSaveable {
        WalletStore(context).entries()
            .flatMap { listOf(it.keyImage, it.txHashHex) }
            .filter { it.isNotEmpty() }
            .joinToString(",")
    }
    val before = remember(atOpen) { atOpen.split(",").filter { it.isNotEmpty() }.toSet() }
    val since = remember(version) {
        // Our own change is an output like any other, and this screen was
        // counting it as generosity: pay for a beer from the takings and the
        // box announced most of the note back as a fresh donation. The poller
        // already knows the answer — a transaction in our send records is
        // ours — and the donation box needs it just as much.
        val ours = WalletStore(context).ourTxids()
        WalletStore(context).entries()
            .filter { it.keyImage.isNotEmpty() && it.keyImage !in before && it.txHashHex !in before }
            .filterNot { it.txHashHex.lowercase() in ours }
            .sumOf { it.amountPxmr }
    }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Avatar(name ?: "?", pic, size = 72)
        Spacer(Modifier.height(10.dp))
        Text(
            if (name != null) stringResource(R.string.donate_support_name, name)
            else stringResource(R.string.donate_support),
            style = MaterialTheme.typography.headlineMedium,
        )
        Spacer(Modifier.height(16.dp))

        if (address == null) {
            Text(stringResource(R.string.donate_no_wallet),
                color = MaterialTheme.colorScheme.error)
            return@Column
        }

        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            FilterChip(
                selected = rail == 0,
                onClick = { rail = 0 },
                label = { Text(stringResource(R.string.donate_tab_ducat)) },
            )
            FilterChip(
                selected = rail == 1,
                onClick = { rail = 1 },
                label = { Text(stringResource(R.string.donate_tab_monero)) },
            )
        }
        Spacer(Modifier.height(12.dp))
        if (rail == 0) {
            when {
                cardUri != null -> QrBlock(cardUri!!)
                cardError != null -> Text(
                    cardError!!, color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
                else -> CatSpinner(
                    Modifier.size(40.dp),
                    tint = MaterialTheme.colorScheme.primary,
                )
            }
            Spacer(Modifier.height(10.dp))
            Text(
                stringResource(R.string.donate_card_hint),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )
        } else {
            QrBlock("monero:$address")
            Spacer(Modifier.height(10.dp))
            Text(
                stringResource(R.string.donate_any_wallet),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )
        }

        if (since > 0) {
            Spacer(Modifier.height(20.dp))
            Text(
                stringResource(R.string.donate_since_opening),
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            val shown = Amounts.show(context, since)
            Text(shown.primary, style = MaterialTheme.typography.displayMedium,
                color = MaterialTheme.ducat.settled)
            shown.secondary?.let {
                Text(it, style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }

        Spacer(Modifier.height(24.dp))
        // §15.9's costs, on the screen where the target is shown rather than
        // in a document nobody at a gig has read. Static rail only: the card
        // rail reuses nothing and links nobody.
        if (rail == 1) Surface(
            color = MaterialTheme.colorScheme.surfaceVariant,
            shape = MaterialTheme.shapes.large,
        ) {
            Text(
                stringResource(R.string.donate_reuse_warning),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(14.dp),
            )
        }
    }
}

/** Recut the standing card an hour before its 12-hour validity runs out. */
private const val CARD_RECUT_MS = 11L * 60 * 60 * 1000
