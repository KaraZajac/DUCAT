package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.Balances
import org.ducatproject.ducat.R

/**
 * Whether the number above this can be trusted yet.
 *
 * A partially scanned wallet reports a *real* balance — every output it lists
 * is genuinely owned — it is just not the whole balance. That is a worse
 * failure than an error, because it looks like an answer. Every screen that
 * shows an amount has to carry this, or the amount is a claim the wallet
 * cannot support.
 *
 * The synced state stays on screen rather than disappearing. "No warning"
 * and "not checked yet" look identical, and only one of them means the
 * balance is complete.
 */
@Composable
fun SyncStatus(b: Balances, modifier: Modifier = Modifier) {
    // Nothing measured yet: no node, no tip, no honest claim either way.
    if (b.tip <= 0L) return

    if (!b.syncing) {
        Row(modifier, verticalAlignment = Alignment.CenterVertically) {
            Text("✓", style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.ducat.settled)
            Spacer(Modifier.width(6.dp))
            Text(
                stringResource(R.string.sync_synced),
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.ducat.settled,
            )
        }
        return
    }

    val context = LocalContext.current
    Column(modifier) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                stringResource(R.string.sync_not_fully),
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.ducat.changePending,
            )
            Spacer(Modifier.weight(1f))
            val pctText = stringResource(R.string.sync_percent, (b.progress * 100).toInt())
            Text(
                buildString {
                    append(pctText)
                    b.secondsLeft?.let { append(" · ${humanDuration(context, it)}") }
                },
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Spacer(Modifier.height(6.dp))
        DucatBar(
            progress = b.progress,
            modifier = Modifier.fillMaxWidth().height(4.dp),
            color = MaterialTheme.ducat.changePending,
        )
        Spacer(Modifier.height(6.dp))
        Text(
            stringResource(R.string.sync_partial_note),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

/**
 * "about 3 minutes", "about 2 hours" — never a false precision. Lives in
 * ui/ rather than Wallet2 because it is a sentence, not protocol: the desk
 * compiles the shared wallet file against a shim with no string resources.
 */
fun humanDuration(context: android.content.Context, secs: Long): String {
    fun plural(res: Int, n: Int) = context.resources.getQuantityString(res, n, n)
    val minutes = kotlin.math.max(1, Math.round(secs / 60.0).toInt())
    val hours = kotlin.math.max(1, Math.round(secs / 3600.0).toInt())
    val days = kotlin.math.max(1, Math.round(secs / 86_400.0).toInt())
    return when {
        secs < 90 -> context.getString(R.string.duration_under_minute)
        secs < 5400 -> plural(R.plurals.duration_minutes, minutes)
        secs < 172_800 -> plural(R.plurals.duration_hours, hours)
        else -> plural(R.plurals.duration_days, days)
    }
}
