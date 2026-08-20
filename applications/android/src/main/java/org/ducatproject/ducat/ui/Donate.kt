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
 * The donation box (§15.11, which defers to §15.9).
 *
 * Deliberately **not** a contact card, and the spec forbids building it as
 * one: a donation target is a standing, public, receive-only address — it
 * takes money and establishes no relationship. That is `TapStatic`'s stance,
 * and it inherits §15.9's admitted limits, which this screen states instead
 * of hiding:
 *
 * - the address is reused, so on the public ledger every donor to this code
 *   is linkable to every other;
 * - a wholly swapped code verifies — a printed sticker is only as honest as
 *   whoever last touched it.
 *
 * What it gets in exchange is the property donations actually need: **any
 * Monero wallet on earth can pay it**, with no DUCAT on the donor's phone.
 */
@Composable
fun DonateScreen() {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val address = remember { WalletStore(context).address() }
    val name = remember { MyProfile(context).name() }
    val pic = remember { MyProfile(context).avatar() }

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

        QrBlock("monero:$address")
        Spacer(Modifier.height(10.dp))
        Text(
            stringResource(R.string.donate_any_wallet),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )

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
        // in a document nobody at a gig has read.
        Surface(
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
