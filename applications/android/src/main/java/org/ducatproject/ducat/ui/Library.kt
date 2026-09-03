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
import androidx.compose.runtime.mutableStateMapOf
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
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.Publications
import org.ducatproject.ducat.R
import org.ducatproject.ducat.Swarm
import org.ducatproject.ducat.saidWhy

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
 * The fetches, owned by the process rather than the screen.
 *
 * A month of a publication is minutes of download, and a Composable's scope
 * dies on the first navigation away — so the screen only *watches* this.
 * Two at a time now that swarm progress is keyed per share: a queue feeds
 * a small pool, and everything past the pool waits its turn visibly.
 */
object LibraryFetch {
    data class Job(val publisherHex: String, val period: String)

    /** How this issue travels (§16.20): a live swarm share, or the shelf —
     *  DHT records that answer even when the publisher's desk is dark. */
    sealed interface Source {
        data class Swarm(val shareKey: String, val digestHex: String) : Source
        data object Shelf : Source
    }

    private const val WIDTH = 2

    /** What each running job's transport is, for progress lookups. */
    private val activeSources = mutableStateMapOf<Job, Source>()

    /** Shelf-path tickers, per job (the swarm path is keyed in Rust). */
    private val shelfTickers = mutableStateMapOf<Job, Swarm.Progress>()

    private val queue = ArrayDeque<Triple<Job, Source, Boolean>>()
    private var runningWorkers = 0

    /** Why each issue's last fetch failed, until its next attempt. Per job:
     *  two run at once, and one slot let the second failure erase the
     *  first row's line — or a fresh download of B blank A's. */
    private val errors = mutableStateMapOf<Job, String>()

    fun errorOf(job: Job): String? = errors[job]

    /** Is this issue being fetched right now? */
    fun activeOn(job: Job): Boolean = activeSources.containsKey(job)

    /** Is it waiting behind the pool? */
    fun queuedOn(job: Job): Boolean =
        synchronized(this) { queue.any { it.first == job } }

    /** Anything at all in flight (the screen's coarse ticker gate). */
    val busy: Boolean
        get() = activeSources.isNotEmpty()

    /** This job's progress, whichever road it is on. */
    fun progressOf(job: Job): Swarm.Progress? {
        val src = activeSources[job] ?: return null
        return when (src) {
            is Source.Swarm -> Swarm.fetchProgress(src.shareKey)
            Source.Shelf -> shelfTickers[job]
        }
    }

    /**
     * Where one issue lands. The last of the three places that check the
     * period id is a name and not a route out of the library — the other
     * two being Publications.absorbKey, which will not file one, and
     * Publications.subscription, which will not hand one back. This one
     * cannot be reasoned past: what runs here is `deleteRecursively` and a
     * fetch that writes, so it asks the filesystem itself.
     */
    fun dirFor(context: Context, publisherHex: String, period: String): File {
        val root = File(context.filesDir, "publications")
        val d = File(root, "$publisherHex/$period")
        require(d.canonicalPath.startsWith(root.canonicalPath + File.separator)) {
            "a period id may not leave the library"
        }
        return d
    }

    /** Bytes on disk for a finished fetch, or null if none landed. */
    fun fetchedBytes(context: Context, publisherHex: String, period: String): Long? {
        // This one runs while the list draws, so a refused id is "nothing
        // downloaded" rather than a screen that will not open.
        val d = runCatching { dirFor(context, publisherHex, period) }.getOrNull() ?: return null
        if (!d.isDirectory) return null
        val files = d.walkTopDown().filter { it.isFile }.toList()
        if (files.isEmpty()) return null
        return files.sumOf { it.length() }
    }

    fun start(context: Context, job: Job, source: Source) {
        enqueue(context, job, source, reseed = false)
    }

    /** Re-serve an already-complete issue: verifies in place, downloads
     *  nothing, stays seeding — the reader as mirror (§16.20's club). */
    fun reseed(context: Context, job: Job, source: Source.Swarm) {
        enqueue(context, job, source, reseed = true)
    }

    private fun enqueue(context: Context, job: Job, source: Source, reseed: Boolean) {
        val app = context.applicationContext
        synchronized(this) {
            if (activeSources.containsKey(job) || queue.any { it.first == job }) return
            queue.addLast(Triple(job, source, reseed))
            if (runningWorkers < WIDTH) {
                runningWorkers++
                Thread { worker(app) }
                    .apply { isDaemon = true; name = "library-fetch-$runningWorkers" }
                    .start()
            }
        }
    }

    private fun worker(app: Context) {
        while (true) {
            val (job, source, reseed) = synchronized(this) {
                val next = queue.removeFirstOrNull() ?: run {
                    runningWorkers--
                    return
                }
                // Claimed under the same lock that dequeues it: between the
                // two, enqueue saw the job neither queued nor active and a
                // second tap could start it twice into one .part directory.
                activeSources[next.first] = next.second
                next
            }
            if (!reseed) errors.remove(job)
            runOne(app, job, source, reseed)
            activeSources.remove(job)
            shelfTickers.remove(job)
            ContactStore.bump()
        }
    }

    private fun runOne(app: Context, job: Job, source: Source, reseed: Boolean) {
        val done = dirFor(app, job.publisherHex, job.period)
        if (reseed) {
            // In place and already whole: verify and stay serving — but
            // only for a publication the reader agreed to help share.
            // Nothing calls this for one they did not (the poller checks
            // too); this is the same question asked where the seeding
            // actually happens.
            if (!Publications.mirroring(app, job.publisherHex)) return
            val share = source as Source.Swarm
            // Verify-only over complete files, so this downloads nothing and
            // the live directory is the right place — but only while it IS
            // complete. A publisher who re-shipped a period leaves a digest
            // this bundle does not match, and then this is an ordinary fetch
            // writing into the directory the reader reads from: the
            // half-overwritten issue the ordinary path below takes a .part
            // dir to avoid. Nothing to verify against means nothing to do.
            if (!done.isDirectory || done.walkTopDown().none { it.isFile }) return
            runCatching {
                Swarm.fetch(
                    share.shareKey, share.digestHex, done.absolutePath,
                    staySeeding = true,
                )
            }.onFailure {
                DucatLog.w("Library", "reseed of '${job.period}': ${it.message}")
            }
            return
        }
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
                        shelfTickers[job] = Swarm.Progress(pos, len, pos >= len && len > 0)
                    }
            }
            done.deleteRecursively()
            check(part.renameTo(done)) { "could not move the download into place" }
            // The reader joins the club — if they said they would. This
            // used to be unconditional, which made downloading an issue a
            // standing commitment to serve it: bandwidth the reader never
            // agreed to, and an announcement to the swarm that this device
            // holds that publication. §16.22 says the same choice out loud
            // for sites; a publication is heavier and says more.
            if (source is Source.Swarm && Publications.mirroring(app, job.publisherHex)) {
                runCatching {
                    Swarm.fetch(
                        source.shareKey, source.digestHex, done.absolutePath,
                        staySeeding = true,
                    )
                }.onFailure {
                    DucatLog.w("Library", "post-fetch seed: ${it.message}")
                }
            }
        } catch (e: Throwable) {
            DucatLog.w(
                "Library",
                "fetch of '${job.period}' from ${job.publisherHex.take(8)}… " +
                    "failed: ${e.message}",
            )
            errors[job] = e.saidWhy() ?: e.javaClass.simpleName
        }
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
    val busy = LibraryFetch.busy

    // Built off the main thread: every row walks its issue's directory on
    // disk to say how big it is, and this ran inside composition on each
    // store bump — a shelf of a few dozen issues stalled the screen for as
    // long as the walks took. The last list stays up while a fresh one is
    // built; before the first, nothing, which beats a wrong "no issues".
    var rows by remember { mutableStateOf<List<IssueRow>?>(null) }
    LaunchedEffect(v, busy) {
        rows = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
            val names = ContactStore(context).all().associate {
                it.personaHex to it.displayName()
            }
            Publications.subscribedPublishers(context).flatMap { pub ->
                val sub = Publications.subscription(context, pub)
                    ?: return@flatMap emptyList()
                // The shelf pair filed once covers every period; a shipment
                // is per-period. Prefer the swarm when both exist — it is
                // the publisher saying this month went by truck.
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
    }

    // A half-second heartbeat while anything is in flight: each row asks
    // LibraryFetch.progressOf for its own job, so two concurrent fetches
    // each show their own bar.
    var tick by remember { mutableStateOf(0L) }
    LaunchedEffect(busy) {
        while (busy) {
            tick = System.currentTimeMillis()
            delay(500)
        }
    }

    // One read for the screen, not two per publisher per frame: see
    // Publications.mutedPublishers.
    val muted = remember(v) { Publications.mutedPublishers(context) }

    val shown = rows ?: return
    if (shown.isEmpty()) {
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
        val grouped = shown.groupBy { it.publisherHex }
        grouped.forEach { (pub, issues) ->
            item(key = "c:$pub") {
                androidx.compose.material3.Card(Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(horizontal = 14.dp, vertical = 8.dp)) {
                        PublisherHeader(pub, issues.first().publisherName, pub in muted)
                        // An unsubscribed shelf keeps what it holds and shows
                        // nothing new — the rows return with Resubscribe,
                        // nothing re-fetched.
                        if (pub !in muted) {
                            issues.forEach { row ->
                                IssueLine(row, tick)
                            }
                        }
                    }
                }
            }
        }
    }
}

/** Hand a fetched issue to whatever can read it. Injected: viewers,
 *  FileProvider and the share sheet are the phone's business (see
 *  MainActivity), and the desk compiles this file without them. */
var libraryOpen: (android.content.Context, String, String) -> Unit = { _, _, _ -> }

@Composable
private fun PublisherHeader(publisherHex: String, publisherName: String?, muted: Boolean) {
    val version by org.ducatproject.ducat.ContactStore.changes.collectAsState()
    val context = LocalContext.current
    var confirm by remember { mutableStateOf(false) }

    if (confirm) {
        androidx.compose.material3.AlertDialog(
            onDismissRequest = { confirm = false },
            title = { Text(stringResource(R.string.library_unsubscribe)) },
            text = {
                Text(
                    stringResource(
                        R.string.library_unsub_confirm,
                        isolate(publisherName ?: "${publisherHex.take(12)}…"),
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
                    // Under the thread's sends, not this header's scope: a
                    // turn of the phone between the tap and the dispatch
                    // cancelled the launch before it ran, and the publisher
                    // went on posting to a cabinet nobody read. A failure
                    // lands on their chat screen as "could not send the
                    // note", if it is up; the log has it either way.
                    val note = context.getString(R.string.library_unsub_note)
                    val store = org.ducatproject.ducat.ContactStore(context)
                    ThreadSends.launch(store, publisherHex, context.getString(R.string.chat_what_note)) {
                        store.all().firstOrNull { it.personaHex == publisherHex }?.let {
                            org.ducatproject.ducat.Mailbox.send(context, it, note)
                        } ?: DucatLog.w("Library", "unsub note: ${publisherHex.take(8)}… is not a contact")
                        null
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
    // The same gift the sites screen asks about, asked here for the same
    // reason and in the same words. Offered only once something has been
    // downloaded — there is nothing to share otherwise, and a checkbox
    // over an empty shelf is a question about nothing.
    val holds = remember(version, publisherHex) {
        Publications.subscription(context, publisherHex)?.third?.keys
            ?.any { LibraryFetch.fetchedBytes(context, publisherHex, it) != null } == true
    }
    if (holds && !muted) {
        val mirroring = remember(version, publisherHex) {
            Publications.mirroring(context, publisherHex)
        }
        Row(
            Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            androidx.compose.material3.Checkbox(
                checked = mirroring,
                onCheckedChange = { on ->
                    Publications.setMirroring(context, publisherHex, on)
                },
            )
            Text(
                stringResource(R.string.library_mirror),
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.weight(1f),
            )
        }
    }
}

@Composable
private fun IssueLine(
    row: IssueRow,
    tick: Long,
) {
    val context = LocalContext.current
    val job = LibraryFetch.Job(row.publisherHex, row.period)
    val mine = LibraryFetch.activeOn(job)
    val queued = !mine && LibraryFetch.queuedOn(job)
    // Re-read on each heartbeat while active; each row shows its own bar.
    val progress = remember(tick, mine) {
        if (mine) LibraryFetch.progressOf(job) else null
    }
    val error = LibraryFetch.errorOf(job)

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
                    stringResource(R.string.library_failed, error),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.error,
                )
            }
        }
        if (row.bytes == null && row.source != null && !mine && !queued) {
            // The queue takes it whenever: two run at once, the rest wait
            // their turn visibly instead of the button going dead.
            Spacer(Modifier.width(12.dp))
            FilledTonalButton(onClick = {
                LibraryFetch.start(context, job, row.source)
            }) {
                Text(stringResource(R.string.library_download))
            }
        } else if (queued) {
            Spacer(Modifier.width(12.dp))
            Text(
                stringResource(R.string.library_queued),
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        } else if (row.bytes != null && !mine) {
            // On this device and readable: the whole point of the fetch.
            // There was no way to open a downloaded issue — the shelf said
            // "205 kB" and stopped.
            androidx.compose.material3.OutlinedButton(onClick = {
                libraryOpen(context, row.publisherHex, row.period)
            }) {
                Text(stringResource(R.string.library_open))
            }
        }
    }
}
