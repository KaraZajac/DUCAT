package org.ducatproject.ducat.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import java.io.File
import java.util.zip.ZipInputStream
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.PageTemplate
import org.ducatproject.ducat.R
import org.ducatproject.ducat.Sites
import org.ducatproject.ducat.saidWhy

/** The staging directory a page is assembled in before it is seeded. */
private fun stagingDir(context: android.content.Context): File =
    File(context.filesDir, "page_staging").apply { mkdirs() }

/**
 * Unpack a picked archive, refusing anything that would write outside it.
 *
 * A zip entry's name is a path chosen by whoever made the file, and
 * `../` in one is the oldest trick there is — the same shape as the
 * period id that could name the reader's keystore, arriving by a
 * different door. The publisher usually made this zip themselves, which
 * is exactly the reasoning that makes it worth checking anyway: "it is
 * my own file" is what somebody thinks right up until it is a file
 * somebody sent them.
 */
private fun unpack(context: android.content.Context, uri: android.net.Uri, into: File): Int {
    into.deleteRecursively()
    into.mkdirs()
    val root = into.canonicalPath + File.separator
    var files = 0
    context.contentResolver.openInputStream(uri).use { raw ->
        ZipInputStream(requireNotNull(raw) { "could not read that file" }).use { zip ->
            while (true) {
                val e = zip.nextEntry ?: break
                if (e.isDirectory) continue
                val out = File(into, e.name)
                require(out.canonicalPath.startsWith(root)) {
                    "that archive tries to write outside the page: ${e.name}"
                }
                out.parentFile?.mkdirs()
                out.outputStream().use { zip.copyTo(it) }
                files++
            }
        }
    }
    require(files > 0) { "that archive is empty" }
    return files
}

/**
 * Where a page is made: a form for somebody who wants an address, and a
 * zip for somebody who already has a bundle.
 *
 * Publishing runs under [ThreadSends] rather than this screen's scope,
 * and that is not tidiness. It seeds a bundle, mints a DHT record and
 * writes a head; a rotation or an incoming call partway through would
 * take the scope with it, and the step that must not be interrupted —
 * committing the owner keypair — is the one that decides whether the
 * address this phone just minted can ever be written to again.
 */
@Composable
fun PagesRoom() {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val mine = remember(version) { Sites.all(context).filter { it.mine } }
    // The page the Address tab has forward, so the two halves of the mode
    // are talking about the same one.
    val front = remember(version, mine) { Sites.frontPage(context) }
    // `starting` is a deliberate "none of them": without it this room could
    // only ever edit a page, never begin a second, because `existing` was
    // whichever one happened to be first and publish() treats a record key
    // as an update. One page per phone was never a rule, only what the UI
    // could reach.
    var starting by rememberSaveable { mutableStateOf(false) }
    val existing = if (starting) null else mine.firstOrNull { it.recordKey == front } ?: mine.firstOrNull()

    // Seeded from what was typed last time, not from the page that was
    // generated out of it. Keyed on the record so a second site starts
    // clean, and *not* on `existing` itself — the row is rewritten by
    // every publish, and re-keying on it would empty the boxes the moment
    // the site came into being.
    val saved = remember(existing?.recordKey, starting) {
        if (starting) null else PageTemplate.fromJson(existing?.page)
    }
    var title by rememberSaveable(existing?.recordKey, starting) {
        mutableStateOf(saved?.title ?: existing?.title.orEmpty())
    }
    var tagline by rememberSaveable(existing?.recordKey, starting) { mutableStateOf(saved?.tagline ?: "") }
    var body by rememberSaveable(existing?.recordKey, starting) { mutableStateOf(saved?.body ?: "") }
    var hours by rememberSaveable(existing?.recordKey, starting) { mutableStateOf(saved?.hours ?: "") }
    var contact by rememberSaveable(existing?.recordKey, starting) { mutableStateOf(saved?.contact ?: "") }
    var word by remember { mutableStateOf<String?>(null) }

    // One fixed key, and this is not cosmetic. Built from the site, it
    // was "page:new" when the publish started and "page:<recordKey>" the
    // instant the store gained the row — so the landing was filed under a
    // key nothing was draining, and a publish that worked perfectly left
    // the room sitting there saying nothing. One page is published at a
    // time here; one key is the truth.
    // Re-seeded when the page being edited *changes*, and only then.
    //
    // Two wrong tools were tried before this one. Keying the
    // rememberSaveables on the selection looks right and survives a
    // rotation, but not a tab switch: the Shell composes one tab at a time
    // (Shells.SaveableStateProvider), so this room leaves composition, its
    // text is saved, and coming back restores words belonging to whichever
    // page was chosen when they were saved. Replacing that with a
    // LaunchedEffect on the selection was worse — an effect runs its block
    // whenever it *enters composition*, keys unchanged or not, so every
    // return from the Address tab silently reverted the form to the last
    // published text with several paragraphs of unsaved typing in it.
    //
    // So the question is not "did the keys change" but "have I already
    // seeded this page in this form", which is a fact that has to outlive
    // composition — hence a saveable marker rather than an effect key.
    var seededFor by rememberSaveable { mutableStateOf<String?>(null) }
    val wants = if (starting) "\u0000new" else existing?.recordKey
    LaunchedEffect(wants) {
        if (seededFor == wants) return@LaunchedEffect
        val p = if (starting) null else PageTemplate.fromJson(existing?.page)
        title = p?.title ?: existing?.title.orEmpty()
        tagline = p?.tagline.orEmpty()
        body = p?.body.orEmpty()
        hours = p?.hours.orEmpty()
        contact = p?.contact.orEmpty()
        seededFor = wants
    }

    val key = "page:publish"
    val tick by ThreadSends.ticks.collectAsState()
    val busy = ThreadSends.inFlight(key)
    LaunchedEffect(tick) {
        for (o in ThreadSends.take(key)) when (o) {
            is ThreadSends.Outcome.Landed -> {
                word = context.getString(R.string.pages_published)
                // A page just made is the one to show; an update leaves the
                // choice alone. Either way this sheet is no longer blank.
                (o.result as? String)?.let { Sites.setFrontPage(context, it) }
                starting = false
                shellTabRequest.value = 0
            }
            is ThreadSends.Outcome.Failed ->
                word = o.error.saidWhy() ?: o.error.javaClass.simpleName
        }
    }

    fun publish(answers: String?, build: (File) -> Unit) {
        word = null
        ThreadSends.launch(ContactStore(context), key, null) {
            val dir = File(stagingDir(context), "current")
            dir.deleteRecursively()
            dir.mkdirs()
            build(dir)
            val made = Sites.publish(context, dir, title.trim(), existing?.recordKey, answers)
            dir.deleteRecursively()
            made.recordKey
        }
    }

    // The escape hatch. A .zip is one file, which the picker can hand
    // over; a directory would need a tree grant and a different contract.
    val pickZip = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        word = null
        ThreadSends.launch(ContactStore(context), key, null) {
            val dir = File(stagingDir(context), "current")
            val n = unpack(context, uri, dir)
            DucatLog.i("Pages", "unpacked $n file(s) from a picked archive")
            val made = Sites.publish(context, dir, title.trim(), existing?.recordKey)
            dir.deleteRecursively()
            made.recordKey
        }
    }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
    ) {
        Text(
            stringResource(
                if (existing == null) R.string.pages_room_new else R.string.pages_room_update,
            ),
            style = MaterialTheme.typography.titleMedium,
        )
        Spacer(Modifier.height(4.dp))
        Text(
            stringResource(R.string.pages_room_body),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(16.dp))

        OutlinedTextField(
            title, { if (it.length <= 80) title = it },
            label = { Text(stringResource(R.string.pages_field_title)) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(8.dp))
        OutlinedTextField(
            tagline, { if (it.length <= 120) tagline = it },
            label = { Text(stringResource(R.string.pages_field_tagline)) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(8.dp))
        OutlinedTextField(
            body, { if (it.length <= 4000) body = it },
            label = { Text(stringResource(R.string.pages_field_body)) },
            minLines = 4,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(8.dp))
        OutlinedTextField(
            hours, { if (it.length <= 400) hours = it },
            label = { Text(stringResource(R.string.pages_field_hours)) },
            minLines = 2,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(8.dp))
        OutlinedTextField(
            contact, { if (it.length <= 400) contact = it },
            label = { Text(stringResource(R.string.pages_field_contact)) },
            minLines = 2,
            modifier = Modifier.fillMaxWidth(),
        )

        Spacer(Modifier.height(16.dp))
        Button(
            enabled = !busy && title.isNotBlank(),
            onClick = {
                val page = PageTemplate.Page(
                    title = title.trim(),
                    tagline = tagline.trim(),
                    body = body.trim(),
                    hours = hours.trim(),
                    contact = contact.trim(),
                )
                publish(PageTemplate.toJson(page)) { dir ->
                    PageTemplate.write(page, dir)
                }
            },
            modifier = Modifier.fillMaxWidth(),
        ) {
            if (busy) {
                CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
            } else {
                Text(
                    stringResource(
                        if (existing == null) R.string.pages_publish else R.string.pages_update,
                    ),
                )
            }
        }

        word?.let {
            Spacer(Modifier.height(10.dp))
            Text(it, style = MaterialTheme.typography.bodySmall)
        }

        if (mine.isNotEmpty()) {
            Spacer(Modifier.height(8.dp))
            OutlinedButton(
                enabled = !busy,
                onClick = { starting = !starting },
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(
                    stringResource(
                        if (starting) R.string.pages_start_cancel else R.string.pages_start_another,
                    ),
                )
            }
        }

        Spacer(Modifier.height(24.dp))
        Card(
            Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
            ),
        ) {
            Column(Modifier.padding(14.dp)) {
                Text(
                    stringResource(R.string.pages_zip_title),
                    style = MaterialTheme.typography.titleSmall,
                )
                Spacer(Modifier.height(4.dp))
                Text(
                    stringResource(R.string.pages_zip_body),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(10.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedButton(
                        enabled = !busy && title.isNotBlank(),
                        onClick = { pickZip.launch(arrayOf("application/zip", "*/*")) },
                    ) { Text(stringResource(R.string.pages_zip_pick)) }
                }
            }
        }
    }
}
