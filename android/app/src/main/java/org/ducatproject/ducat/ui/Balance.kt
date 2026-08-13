package org.ducatproject.ducat.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.ducatproject.ducat.Capacity
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
) {
    Card(
        modifier = Modifier.fillMaxWidth().padding(16.dp),
        shape = RoundedCornerShape(20.dp),
    ) {
        Column(Modifier.padding(20.dp)) {
            // Spendable *now* — not the total. The total is the number that
            // gets someone declined at a counter.
            Text("Ready to spend", style = MaterialTheme.typography.labelLarge)
            // Through the shared formatter, so the currency switch reaches the
            // one number people actually look at. It bypassed it before, which
            // is how the headline figure ended up unitless.
            val ctx = androidx.compose.ui.platform.LocalContext.current
            val shown = org.ducatproject.ducat.Amounts.show(ctx, spendablePxmr)
            Text(
                shown.primary,
                fontSize = 40.sp,
                fontWeight = FontWeight.Bold,
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
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            Spacer(Modifier.height(4.dp))

            // §17.2 forbids an exact promise, so the wording carries the
            // approximation rather than hiding it behind a precise-looking digit.
            Text(
                notesPhrase(float.unlockedOutputs, capacity.approxPayments),
                style = MaterialTheme.typography.bodyMedium,
            )

            if (locked != null && float.lockedPxmr > 0) {
                Spacer(Modifier.height(12.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(
                        Modifier.size(8.dp)
                            // Change coming back has its own colour because it
                            // carries a meaning, not a mood: it is a consequence
                            // of having spent, and not a warning.
                            .background(MaterialTheme.ducat.changePending, RoundedCornerShape(4.dp))
                    )
                    Spacer(Modifier.width(8.dp))
                    // "Change coming back" rather than "locked": the lock is a
                    // consequence of having spent, and naming it that way is both
                    // accurate and not alarming.
                    Text(
                        "$locked in change, back in ${minutesFor(float.blocksToUnlock)}",
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
            }

            // §17.2 requires warning *before* the count reaches zero, not at the
            // counter: "a client that funds a float and immediately offers to
            // transact will fail at the curb with a full balance on screen."
            if (float.unlockedOutputs == 0 || capacity.approxPayments <= 2) {
                Spacer(Modifier.height(16.dp))
                Surface(
                    color = MaterialTheme.colorScheme.errorContainer,
                    shape = RoundedCornerShape(12.dp),
                ) {
                    Column(Modifier.padding(12.dp)) {
                        Text(
                            if (float.unlockedOutputs == 0)
                                "You can't pay right now"
                            else
                                "Running low on notes",
                            fontWeight = FontWeight.SemiBold,
                        )
                        Text(
                            if (float.unlockedOutputs == 0)
                                "Your money is here, but it's all tied up as change. " +
                                    "Break a note to spend again."
                            else
                                "Break a note now so you're not caught out at a counter.",
                            style = MaterialTheme.typography.bodySmall,
                        )
                        Spacer(Modifier.height(8.dp))
                        Button(onClick = onTopUp) { Text("Break a note") }
                    }
                }
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
internal fun notesPhrase(outputs: Int, approxPayments: Int): String {
    val notes = if (outputs == 1) "1 note" else "$outputs notes"
    // `approxPayments` answers "how many spends in a row without waiting for
    // change" — it is §17.2's planning figure, and floor(1/1.5) is 0. Using it
    // to answer "can I pay at all" told someone holding a perfectly spendable
    // note that they could not spend it.
    return when {
        outputs == 0 -> "nothing unlocked yet"
        approxPayments == 0 -> "$notes — enough for one payment, then a wait for change"
        approxPayments == 1 -> "$notes, so about one more payment"
        else -> "$notes, so about $approxPayments more payments"
    }
}


/** Monero stagenet and mainnet both target two-minute blocks. */
internal fun minutesFor(blocks: Int): String = when {
    blocks <= 0 -> "any moment"
    blocks * 2 < 60 -> "about ${blocks * 2} minutes"
    else -> "about ${(blocks * 2) / 60} hours"
}
