package org.ducatproject.ducat.ui

import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.ducatproject.ducat.Capacity
import org.ducatproject.ducat.R
import org.ducatproject.ducat.Float as DucatFloat
import org.ducatproject.ducat.Money

/**
 * The screen with no PayPal equivalent, and the one most likely to lose a user.
 *
 * PayPal shows a number you can spend. We cannot, honestly: §17.2 makes capacity
 * a **count of unlocked outputs**, a payment consumes at least one whole output,
 * and change returns locked for ten blocks. Someone who sees a balance, taps, and
 * is declined will not forgive it — and would be right not to.
 *
 * The framing is notes in a wallet, which is not a simplification of Monero's
 * output model but a description of it: forty dollars as four tens is four
 * purchases, and change takes twenty minutes. People already understand this,
 * because physical cash works the same way — and it makes the ten-block lock
 * legible as *waiting for change* rather than as something inexplicable.
 */
@Composable
fun BalanceCard(
    spendablePxmr: Long,
    capacity: Capacity,
    float: DucatFloat,
    locked: Money?,
    onTopUp: () -> Unit,
    /** The scan behind the number above. Null only where there is no wallet. */
    sync: org.ducatproject.ducat.Balances? = null,
) {
    // The number lives on the page, not in a card. Venmo and PayPal both do
    // this and it is why their home screens read as calm: a card is a box you
    // evaluate, a figure on the page is a fact you own. The card below holds
    // the qualifications, which are details and deserve a box.
    val ctx = androidx.compose.ui.platform.LocalContext.current
    val shown = org.ducatproject.ducat.Amounts.show(ctx, spendablePxmr)
    Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp)) {
        Spacer(Modifier.height(20.dp))
        Text(
            stringResource(R.string.balance_ready_to_spend),
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(2.dp))
        Text(
            shown.primary,
            style = MaterialTheme.typography.displayLarge,
            // Tapping the amount flips the unit. The balance is where
            // someone asks "how much is that really", so it is where the
            // answer should be one tap away.
            modifier = Modifier.clickable(enabled = shown.secondary != null) {
                org.ducatproject.ducat.Amounts.setPreferFiat(
                    ctx, !org.ducatproject.ducat.Amounts.preferFiat(ctx),
                )
            },
        )
        shown.secondary?.let {
            Text(
                it,
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        // Directly under the figure it qualifies. A caveat placed further
        // down is a caveat someone reads after deciding.
        sync?.let {
            Spacer(Modifier.height(12.dp))
            SyncStatus(it)
        }
        Spacer(Modifier.height(20.dp))
    }

    Card(modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
        Column(Modifier.padding(20.dp)) {
            // §17.2 forbids an exact promise, so the wording carries the
            // approximation rather than hiding it behind a precise-looking digit.
            Text(
                notesPhrase(ctx, float.unlockedOutputs, capacity.approxPayments),
                style = MaterialTheme.typography.bodyMedium,
            )

            if (locked != null && float.lockedPxmr > 0) {
                Spacer(Modifier.height(12.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(
                        Modifier.size(8.dp)
                            // Money on its way has its own colour because it
                            // carries a meaning, not a mood: it is arriving,
                            // not a warning.
                            .background(MaterialTheme.ducat.changePending, RoundedCornerShape(4.dp))
                    )
                    Spacer(Modifier.width(8.dp))
                    // "On the way" rather than "locked" — and rather than "in
                    // change", which is what this used to say. Every unspent
                    // output inside the lock window counts here, and a payment
                    // somebody just made to you is one of them: `balances()`
                    // sees outputs, not their origin, and telling a person who
                    // has just been paid that their money is "change" is
                    // nonsense to them. Wording that is true either way costs
                    // nothing; separating the two would cost a chain fetch per
                    // output, which is the Ledger's job and not a balance's.
                    Text(
                        stringResource(
                            R.string.balance_arriving,
                            locked.toString(),
                            minutesFor(ctx, float.blocksToUnlock),
                        ),
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
            }
        }
    }

    // An empty wallet is not tied-up change. "Your money is here, but it's
    // all locked" over a zero balance is a lie the first-run screen used to
    // tell — the card fires only when money actually exists but none of it
    // is spendable, or when the spendable supply is running thin. A wallet
    // with nothing in it says nothing here; the Send/Receive tab is where a
    // first deposit comes from.
    val hasMoney = float.unlockedOutputs > 0 || float.lockedPxmr > 0
    val allLocked = float.unlockedOutputs == 0 && float.lockedPxmr > 0
    if (hasMoney && (allLocked || capacity.approxPayments <= 2)) {
        Spacer(Modifier.height(12.dp))
        Surface(
            color = MaterialTheme.colorScheme.errorContainer,
            shape = MaterialTheme.shapes.large,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        ) {
            Column(Modifier.padding(20.dp)) {
                Text(
                    if (allLocked) stringResource(R.string.balance_all_locked_title)
                    else stringResource(R.string.balance_running_low_title),
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onErrorContainer,
                )
                Spacer(Modifier.height(4.dp))
                Text(
                    if (allLocked) stringResource(R.string.balance_all_locked_body)
                    else stringResource(R.string.balance_running_low_body),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onErrorContainer,
                )
                Spacer(Modifier.height(12.dp))
                Button(onClick = onTopUp) { Text(stringResource(R.string.balance_top_up_action)) }
            }
        }
    }
}

/**
 * The sentence a user reads instead of a number they would misread.
 *
 * Deliberately vague where §17.2 says the truth is vague. The count of outputs
 * is an upper bound on payments, not an equality — six bought four in the drain
 * test — so the copy says "about".
 */
internal fun notesPhrase(context: Context, outputs: Int, approxPayments: Int): String {
    val notes = context.resources.getQuantityString(R.plurals.balance_notes, outputs, outputs)
    // `approxPayments` answers "how many spends in a row without waiting for
    // change" — it is §17.2's planning figure, and floor(1/1.5) is 0. Using it
    // to answer "can I pay at all" told someone holding a perfectly spendable
    // note that they could not spend it.
    return when {
        outputs == 0 -> context.getString(R.string.balance_nothing_unlocked)
        approxPayments == 0 -> context.getString(R.string.balance_notes_then_wait, notes)
        approxPayments == 1 -> context.getString(R.string.balance_notes_one_more, notes)
        else -> context.getString(R.string.balance_notes_more, notes, approxPayments)
    }
}


/** Monero stagenet and mainnet both target two-minute blocks. */
internal fun minutesFor(context: Context, blocks: Int): String = when {
    blocks <= 0 -> context.getString(R.string.balance_unlock_any_moment)
    blocks * 2 < 60 -> context.resources.getQuantityString(
        R.plurals.balance_unlock_minutes, blocks * 2, blocks * 2,
    )
    else -> context.resources.getQuantityString(
        R.plurals.balance_unlock_hours, (blocks * 2) / 60, (blocks * 2) / 60,
    )
}
