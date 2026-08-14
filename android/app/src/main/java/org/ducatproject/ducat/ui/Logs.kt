package org.ducatproject.ducat.ui

import android.content.Intent
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.DeleteOutline
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.ducatproject.ducat.DucatLog
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * What the app has been doing.
 *
 * Exists because every problem in this app so far has been diagnosed by
 * guessing: the reason was in logcat, which needs a cable and a desktop, and
 * the person seeing the problem had neither. One tap to copy turns "I see
 * zeros" into a line someone can read.
 *
 * Secrets are stripped when entries are written, not here — a redaction that
 * only happens on display is one that a second display path forgets.
 */
@Composable
fun LogsScreen() {
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current
    val version by DucatLog.changes.collectAsState()
    // A filter, because a day's field test buries the one red line under four
    // hundred quiet ones, and scrolling for it is how it gets missed.
    var filter by remember { mutableStateOf<DucatLog.Level?>(null) }
    val all = remember(version) { DucatLog.snapshot().asReversed() }
    val entries = remember(all, filter) {
        filter?.let { f -> all.filter { it.level == f } } ?: all
    }
    val listState = rememberLazyListState()
    var copied by remember { mutableStateOf(false) }

    LaunchedEffect(copied) {
        if (copied) { kotlinx.coroutines.delay(1500); copied = false }
    }

    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            val warns = all.count { it.level == DucatLog.Level.Warn }
            val errors = all.count { it.level == DucatLog.Level.Error }
            Text(
                "${all.size} line(s)" +
                    (if (warns > 0) " · $warns warn" else "") +
                    (if (errors > 0) " · $errors error" else ""),
                style = MaterialTheme.typography.bodySmall,
                color = if (errors > 0) MaterialTheme.colorScheme.error
                else MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.weight(1f))
            IconButton(onClick = {
                clipboard.setText(AnnotatedString(DucatLog.asText()))
                copied = true
            }) { Icon(Icons.Filled.ContentCopy, "Copy all") }
            IconButton(onClick = {
                val i = Intent(Intent.ACTION_SEND).apply {
                    type = "text/plain"
                    putExtra(Intent.EXTRA_TEXT, DucatLog.asText())
                }
                context.startActivity(Intent.createChooser(i, "Send logs"))
            }) { Icon(Icons.Filled.Share, "Share") }
            IconButton(onClick = { DucatLog.clear() }) {
                Icon(Icons.Filled.DeleteOutline, "Clear")
            }
        }

        if (copied) {
            Text(
                "Copied. Keys and message text are never in here.",
                Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.ducat.settled,
            )
        }
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            @Composable
            fun chip(label: String, level: DucatLog.Level?) = FilterChip(
                selected = filter == level,
                onClick = { filter = if (filter == level) null else level },
                label = { Text(label, style = MaterialTheme.typography.labelSmall) },
            )
            chip("All", null)
            chip("Info", DucatLog.Level.Info)
            chip("Warn", DucatLog.Level.Warn)
            chip("Error", DucatLog.Level.Error)
        }
        HorizontalDivider()

        if (entries.isEmpty()) {
            Box(Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
                Text(
                    "Nothing logged yet. Activity appears here as the app syncs, " +
                        "sends and receives.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            return
        }

        val clock = remember { SimpleDateFormat("HH:mm:ss", Locale.US) }
        LazyColumn(Modifier.fillMaxSize(), state = listState) {
            items(entries) { e ->
                Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 3.dp)) {
                    Text(
                        clock.format(Date(e.at)),
                        fontFamily = FontFamily.Monospace,
                        fontSize = 11.sp,
                        color = MaterialTheme.colorScheme.outline,
                    )
                    Spacer(Modifier.width(8.dp))
                    Text(
                        "${e.tag.removePrefix("Ducat")}: ${e.message}",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 11.sp,
                        color = when (e.level) {
                            DucatLog.Level.Error -> MaterialTheme.colorScheme.error
                            DucatLog.Level.Warn -> MaterialTheme.ducat.lowCapacity
                            DucatLog.Level.Info -> MaterialTheme.colorScheme.onSurface
                        },
                    )
                }
            }
        }
    }
}
