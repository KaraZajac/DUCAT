package org.ducatproject.ducat.ui

import android.content.Context
import android.text.format.Formatter
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.LocalLibrary
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import java.io.File
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.Publications
import org.ducatproject.ducat.R
import org.ducatproject.ducat.Swarm

/**
 * The subscriber's library (§16.20): every publication key the cabinet has
 * filed, grouped by publisher, with the downloads beside them.
 *
 * The cabinet — not the chat — is what this screen reads, because the
 * cabinet outlives the thread: delete the conversation and the issues you
 * hold are still yours. A period appears the moment its key is filed;
 * the Download button appears only when a shipment (share key + index
 * digest, together on the wire or not at all) rode the same manifest.
 */

/**
 * The one fetch, owned by the process rather than the screen.
 *
 * A month of a publication is minutes of download, and a Composable's scope
 * dies on the first navigation away — so the screen only *watches* this.
 * One at a time is the client contract ([Swarm.fetchProgress] is a single
 * slot); the button simply doesn't offer while one runs.
 */
object LibraryFetch {
    data class Job(val publisherHex: String, val period: String)

    /** How this issue travels (§16.20): a live swarm share, or the shelf —
     *  DHT records that answer even when the publisher's desk is dark. */
    sealed interface Source {
        data class Swarm(val shareKey: String, val digestHex: String) : Source
        data object Shelf : Source
    }

    var current by mutableStateOf<Job?>(null)
        private set
    var lastError by mutableStateOf<Pair<Job, String>?>(null)
        private set

    /** The shelf path's own ticker; the swarm path reports through
     *  [Swarm.fetchProgress] as before. */
    var shelfProgress by mutableStateOf<Swarm.Progress?>(null)
        private set

    fun dirFor(context: Context, publisherHex: String, period: String): File =
        File(context.filesDir, "publications/$publisherHex/$period")

    /** Bytes on disk for a finished fetch, or null if none landed. */
    fun fetchedBytes(context: Context, publisherHex: String, period: String): Long? {
        val d = dirFor(context, publisherHex, period)
        if (!d.isDirectory) return null
        val files = d.walkTopDown().filter { it.isFile }.toList()
        if (files.isEmpty()) return null
        return files.sumOf { it.length() }
    }

    fun start(context: Context, job: Job, source: Source) {
        synchronized(this) {
            if (current != null) return
            current = job
        }
        lastError = null
        shelfProgress = null
        val app = context.applicationContext
        Thread {
            val done = dirFor(app, job.publisherHex, job.period)
            // Fetch lands in a .part sibling and moves into place whole, so
            // "the directory exists" always means "every piece verified".
            // The .part survives a failure on purpose: the swarm engine
            // checks pieces on disk, so a retry resumes instead of starting
            // over; the shelf just rewrites, its issues being small.
            val part = File(done.parentFile, done.name + ".part")
            try {
                part.mkdirs()
                when (source) {
                    is Source.Swarm ->
                        Swarm.fetch(source.shareKey, source.digestHex, part.absolutePath)
                    Source.Shelf ->
                        Publications.fetchShelf(
                            app, job.publisherHex, job.period, part,
                        ) { pos, len ->
                            shelfProgress = Swarm.Progress(pos, len, pos >= len && len > 0)
                        }
                }
                done.deleteRecursively()
                check(part.renameTo(done)) { "could not move the download into place" }
            } catch (e: Throwable) {
                DucatLog.w(
                    "Library",
                    "fetch of '${job.period}' from ${job.publisherHex.take(8)}… " +
                        "failed: ${e.message}",
                )
                lastError = job to (e.message ?: e.javaClass.simpleName)
            } finally {
                current = null
                shelfProgress = null
                ContactStore.bump()
            }
        }.apply { isDaemon = true; name = "library-fetch" }.start()
    }
}

private data class IssueRow(
    val publisherHex: String,
    val publisherName: String?,
    val period: String,
    val source: LibraryFetch.Source?,
    val bytes: Long?,
)

@Composable
fun LibrarySection() {
    val context = LocalContext.current
    val v by ContactStore.changes.collectAsState()
    val fetching = LibraryFetch.current

    val rows = remember(v, fetching) {
        val names = ContactStore(context).all().associate {
            it.personaHex to it.displayName()
        }
        Publications.subscribedPublishers(context).flatMap { pub ->
            val sub = Publications.subscription(context, pub)
                ?: return@flatMap emptyList()
            // The shelf pair filed once covers every period; a shipment is
            // per-period. Prefer the swarm when both exist — it is the
            // publisher saying this month went by truck.
            val hasShelf = sub.first != null && sub.second != null
            sub.third.keys.sortedDescending().map { period ->
                val ship = Publications.shipment(context, pub, period)
                IssueRow(
                    publisherHex = pub,
                    publisherName = names[pub],
                    period = period,
                    source = when {
                        ship != null -> LibraryFetch.Source.Swarm(ship.first, ship.second)
                        hasShelf -> LibraryFetch.Source.Shelf
                        else -> null
                    },
                    bytes = LibraryFetch.fetchedBytes(context, pub, period),
                )
            }
        }.sortedWith(
            compareBy({ it.publisherName ?: it.publisherHex }, { it.publisherHex }),
        )
    }

    // The running fetch's progress: the shelf path reports its own; the
    // swarm path through the same poll the desk proof used.
    var progress by remember { mutableStateOf<Swarm.Progress?>(null) }
    LaunchedEffect(fetching) {
        progress = null
        while (fetching != null) {
            progress = LibraryFetch.shelfProgress ?: Swarm.fetchProgress()
            delay(500)
        }
    }

    if (rows.isEmpty()) {
        Box(Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Icon(
                    Icons.Filled.LocalLibrary, null,
                    Modifier.size(48.dp),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(12.dp))
                Text(
                    stringResource(R.string.library_empty),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        return
    }

    // One card per publication — the same card language as every other
    // list in the app, and it demotes Unsubscribe from the loudest thing
    // on the shelf to a corner of the cabinet it belongs to.
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = androidx.compose.foundation.layout.PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        val grouped = rows.groupBy { it.publisherHex }
        grouped.forEach { (pub, issues) ->
            item(key = "c:$pub") {
                androidx.compose.material3.Card(Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(horizontal = 14.dp, vertical = 8.dp)) {
                        PublisherHeader(pub, issues.first().publisherName)
                        // An unsubscribed shelf keeps what it holds and shows
                        // nothing new — the rows return with Resubscribe,
                        // nothing re-fetched.
                        if (!Publications.isMuted(context, pub)) {
                            issues.forEach { row ->
                                IssueLine(row, fetching, progress)
                            }
                        }
                    }
                }
            }
        }
    }
}

/** Hand a fetched issue to whatever can read it — view first, share sheet
 *  when nothing on the device claims the type. */
private fun openIssue(context: android.content.Context, publisherHex: String, period: String) {
    val dir = java.io.File(context.filesDir, "publications/$publisherHex/$period")
    val file = dir.walkTopDown().filter { it.isFile && !it.name.endsWith(".part") }
        .maxByOrNull { it.length() } ?: return
    val uri = androidx.core.content.FileProvider.getUriForFile(
        context, "${context.packageName}.backups", file,
    )
    val mime = android.webkit.MimeTypeMap.getSingleton()
        .getMimeTypeFromExtension(file.extension.lowercase()) ?: "*/*"
    val view = android.content.Intent(android.content.Intent.ACTION_VIEW).apply {
        setDataAndType(uri, mime)
        addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    runCatching { context.startActivity(view) }.onFailure {
        // Nothing installed reads this type: the share sheet always opens,
        // and sending it to yourself is still a way to read it elsewhere.
        val send = android.content.Intent(android.content.Intent.ACTION_SEND).apply {
            type = mime
            putExtra(android.content.Intent.EXTRA_STREAM, uri)
            addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        runCatching {
            context.startActivity(
                android.content.Intent.createChooser(send, file.name),
            )
        }
    }
}

@Composable
private fun PublisherHeader(publisherHex: String, publisherName: String?) {
    val context = LocalContext.current
    val muted = Publications.isMuted(context, publisherHex)
    var confirm by remember { mutableStateOf(false) }
    val scope = androidx.compose.runtime.rememberCoroutineScope()

    if (confirm) {
        androidx.compose.material3.AlertDialog(
            onDismissRequest = { confirm = false },
            title = { Text(stringResource(R.string.library_unsubscribe)) },
            text = {
                Text(
                    stringResource(
                        R.string.library_unsub_confirm,
                        publisherName ?: "${publisherHex.take(12)}…",
                    ),
                )
            },
            confirmButton = {
                androidx.compose.material3.TextButton(onClick = {
                    confirm = false
                    Publications.setMuted(context, publisherHex, true)
                    // The courtesy note, protocol-written in OUR language
                    // (the bill placeholder's rule): the publisher is told
                    // in words, and their roster is theirs to tidy.
                    val note = context.getString(R.string.library_unsub_note)
                    scope.launch(kotlinx.coroutines.Dispatchers.IO) {
                        runCatching {
                            org.ducatproject.ducat.ContactStore(context).all()
                                .firstOrNull { it.personaHex == publisherHex }
                                ?.let { org.ducatproject.ducat.Mailbox.send(context, it, note) }
                        }.onFailure {
                            DucatLog.w("Library", "unsub note: ${it.message}")
                        }
                    }
                }) { Text(stringResource(R.string.library_unsubscribe)) }
            },
            dismissButton = {
                androidx.compose.material3.TextButton(onClick = { confirm = false }) {
                    Text(stringResource(R.string.library_unsub_keep))
                }
            },
        )
    }

    Row(
        Modifier.fillMaxWidth().padding(top = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(
                publisherName ?: "${publisherHex.take(12)}…",
                style = MaterialTheme.typography.titleMedium,
            )
            if (publisherName == null) {
                // The cabinet outlives the thread; say so rather than
                // showing a bare key with no explanation.
                Text(
                    stringResource(R.string.library_gone),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (muted) {
                Text(
                    stringResource(R.string.library_unsubscribed),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        androidx.compose.material3.TextButton(onClick = {
            if (muted) Publications.setMuted(context, publisherHex, false)
            else confirm = true
        }) {
            Text(
                stringResource(
                    if (muted) R.string.library_resubscribe
                    else R.string.library_unsubscribe,
                ),
            )
        }
    }
}

@Composable
private fun IssueLine(
    row: IssueRow,
    fetching: LibraryFetch.Job?,
    progress: Swarm.Progress?,
) {
    val context = LocalContext.current
    val mine = fetching?.publisherHex == row.publisherHex &&
        fetching?.period == row.period
    val error = LibraryFetch.lastError?.takeIf {
        it.first.publisherHex == row.publisherHex && it.first.period == row.period
    }

    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(
                stringResource(R.string.library_issue, row.period),
                style = MaterialTheme.typography.bodyLarge,
            )
            when {
                mine -> {
                    val p = progress
                    if (p != null && p.length > 0) {
                        LinearProgressIndicator(
                            progress = {
                                (p.position.toFloat() / p.length.toFloat())
                                    .coerceIn(0f, 1f)
                            },
                            Modifier.fillMaxWidth().padding(top = 4.dp, bottom = 2.dp),
                        )
                        Text(
                            stringResource(
                                R.string.library_progress,
                                Formatter.formatShortFileSize(
                                    context, p.position.coerceAtLeast(0),
                                ),
                                Formatter.formatShortFileSize(context, p.length.toLong()),
                            ),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    } else {
                        Text(
                            stringResource(R.string.library_starting),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                row.bytes != null -> Text(
                    stringResource(
                        R.string.library_on_device,
                        Formatter.formatShortFileSize(context, row.bytes),
                    ),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                row.source == null -> Text(
                    stringResource(R.string.library_no_shipment),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (error != null && !mine) {
                Text(
                    stringResource(R.string.library_failed, error.second),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.error,
                )
            }
        }
        if (row.bytes == null && row.source != null && fetching == null) {
            Spacer(Modifier.width(12.dp))
            FilledTonalButton(onClick = {
                LibraryFetch.start(
                    context,
                    LibraryFetch.Job(row.publisherHex, row.period),
                    row.source,
                )
            }) {
                Text(stringResource(R.string.library_download))
            }
        } else if (row.bytes != null && !mine) {
            // On this device and readable: the whole point of the fetch.
            // There was no way to open a downloaded issue — the shelf said
            // "205 kB" and stopped.
            androidx.compose.material3.OutlinedButton(onClick = {
                openIssue(context, row.publisherHex, row.period)
            }) {
                Text(stringResource(R.string.library_open))
            }
        }
    }
}
