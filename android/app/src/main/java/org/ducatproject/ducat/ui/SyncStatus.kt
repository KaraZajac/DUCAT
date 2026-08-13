package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.Balances
import org.ducatproject.ducat.humanDuration

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
                "Synced",
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.ducat.settled,
            )
        }
        return
    }

    Column(modifier) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                "Not fully synced",
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.ducat.changePending,
            )
            Spacer(Modifier.weight(1f))
            Text(
                buildString {
                    append("${(b.progress * 100).toInt()}%")
                    b.secondsLeft?.let { append(" · ${humanDuration(it)}") }
                },
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Spacer(Modifier.height(6.dp))
        LinearProgressIndicator(
            progress = { b.progress },
            modifier = Modifier.fillMaxWidth().height(4.dp).clip(RoundedCornerShape(2.dp)),
            color = MaterialTheme.ducat.changePending,
        )
        Spacer(Modifier.height(6.dp))
        Text(
            "This balance is only what has been read so far — it may be low " +
                "until the scan finishes.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
