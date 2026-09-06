package org.ducatproject.ducat.ui

import android.content.Intent
import android.graphics.BitmapFactory
import android.net.Uri
import android.text.format.DateUtils
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.OpenInNew
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Card
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.Home
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.R
import org.ducatproject.ducat.Releases
import org.ducatproject.ducat.SiteViewerActivity
import uniffi.ducat_mobile.FeedEntry

/**
 * §16.23's feed: the posts of everyone kept, newest first, mine among
 * them — and the Post button that adds to mine.
 */
@Composable
fun FeedSection() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var rows by remember { mutableStateOf<List<FeedEntry>>(emptyList()) }
    var loaded by remember { mutableStateOf(false) }
    var composing by rememberSaveable { mutableStateOf(false) }
    var text by rememberSaveable { mutableStateOf("") }
    var media by remember { mutableStateOf<List<Uri>>(emptyList()) }
    var files by remember { mutableStateOf<List<Uri>>(emptyList()) }
    var busy by remember { mutableStateOf(false) }
    var message by remember { mutableStateOf<String?>(null) }
    var confirmDelete by remember { mutableStateOf<FeedEntry?>(null) }
    var myPosts by remember { mutableStateOf(0) }
    val me = remember { PersonaStore(context).worn() }

    suspend fun refresh() {
        val t = withContext(Dispatchers.IO) { runCatching { Home.timeline(context) }.getOrDefault(emptyList()) }
        rows = t
        myPosts = withContext(Dispatchers.IO) { runCatching { Home.myHomeView(context).third }.getOrDefault(0) }
        loaded = true
    }
    LaunchedEffect(Unit) { refresh() }

    val pickPhotos = rememberLauncherForActivityResult(ActivityResultContracts.GetMultipleContents()) { uris ->
        media = (media + uris).take(8)
    }
    val pickFiles = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        for (u in uris) runCatching { context.contentResolver.takePersistableUriPermission(u, Intent.FLAG_GRANT_READ_URI_PERMISSION) }
        files = (files + uris).take(8)
    }

    confirmDelete?.let { e ->
        AlertDialog(
            onDismissRequest = { confirmDelete = null },
            title = { Text(stringResource(R.string.feed_delete_post_title)) },
            text = { Text(stringResource(R.string.feed_delete_post_body)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmDelete = null
                    scope.launch {
                        withContext(Dispatchers.IO) { runCatching { Home.deletePost(context, e.post.id) }.onFailure { DucatLog.w("Feed", "delete: ${it.message}"); message = context.getString(R.string.feed_failed) } }
                        refresh()
                    }
                }) { Text(stringResource(R.string.feed_delete_post)) }
            },
            dismissButton = { TextButton(onClick = { confirmDelete = null }) { Text(stringResource(R.string.common_cancel)) } },
        )
    }

    LazyColumn(Modifier.fillMaxSize(), contentPadding = androidx.compose.foundation.layout.PaddingValues(16.dp)) {
        item {
            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
                Text(stringResource(R.string.section_feed), style = MaterialTheme.typography.headlineSmall, modifier = Modifier.weight(1f))
                IconButton(onClick = {
                    scope.launch {
                        busy = true
                        val n = withContext(Dispatchers.IO) { runCatching { Home.refreshFeeds(context) }.getOrDefault(0) }
                        refresh()
                        message = if (n > 0) context.resources.getQuantityString(R.plurals.feed_new_editions, n, n) else context.getString(R.string.feed_nothing_new)
                        busy = false
                    }
                }, enabled = !busy) { Icon(Icons.Filled.Refresh, stringResource(R.string.feed_refresh)) }
                Button(onClick = { composing = !composing }) {
                    Text(if (composing) stringResource(R.string.common_cancel) else stringResource(R.string.feed_post))
                }
            }
            Text(
                if (myPosts > 0) context.resources.getQuantityString(R.plurals.feed_home_posts, myPosts, myPosts) else stringResource(R.string.feed_no_home_yet),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = 4.dp, bottom = 8.dp),
            )
            message?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(bottom = 8.dp)) }
        }
        if (composing) {
            item {
                Card(Modifier.fillMaxWidth().padding(bottom = 12.dp)) {
                    Column(Modifier.padding(14.dp)) {
                        OutlinedTextField(
                            value = text,
                            onValueChange = { text = it.take(4000) },
                            placeholder = { Text(stringResource(R.string.feed_post_hint)) },
                            minLines = 3,
                            modifier = Modifier.fillMaxWidth(),
                        )
                        if (media.isNotEmpty() || files.isNotEmpty()) {
                            Row(Modifier.padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                if (media.isNotEmpty()) AssistChip(onClick = { media = emptyList() }, label = { Text(context.resources.getQuantityString(R.plurals.feed_n_pictures, media.size, media.size)) })
                                if (files.isNotEmpty()) AssistChip(onClick = { files = emptyList() }, label = { Text(context.resources.getQuantityString(R.plurals.feed_n_files, files.size, files.size)) })
                            }
                        }
                        Row(Modifier.padding(top = 10.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                            OutlinedButton(onClick = { pickPhotos.launch("image/*") }, enabled = !busy) { Text(stringResource(R.string.feed_add_photos)) }
                            OutlinedButton(onClick = { pickFiles.launch(arrayOf("*/*")) }, enabled = !busy) { Text(stringResource(R.string.feed_add_files)) }
                            Spacer(Modifier.weight(1f))
                            Button(
                                enabled = !busy && (text.isNotBlank() || media.isNotEmpty() || files.isNotEmpty()),
                                onClick = {
                                    scope.launch {
                                        busy = true
                                        message = null
                                        val r = withContext(Dispatchers.IO) { runCatching { Home.post(context, text, media, files) } }
                                        r.onSuccess { text = ""; media = emptyList(); files = emptyList(); composing = false; message = context.getString(R.string.feed_posted) }
                                            .onFailure { DucatLog.w("Feed", "post: ${it.message}"); message = context.getString(R.string.feed_failed) }
                                        refresh()
                                        busy = false
                                    }
                                },
                            ) { Text(if (busy) stringResource(R.string.feed_posting) else stringResource(R.string.feed_post)) }
                        }
                        Text(stringResource(R.string.feed_post_note), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(top = 8.dp))
                    }
                }
            }
        }
        if (loaded && rows.isEmpty()) {
            item { Text(stringResource(R.string.feed_empty), color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(vertical = 24.dp)) }
        }
        items(rows, key = { it.persona + ":" + it.post.id }) { e ->
            FeedCard(e, mine = e.persona == me, onOpen = {
                scope.launch {
                    val key = withContext(Dispatchers.IO) { runCatching { Home.keyOf(e.persona) }.getOrNull() } ?: return@launch
                    context.startActivity(Intent(context, SiteViewerActivity::class.java).putExtra("record", key).putExtra("page", "posts/${e.post.id}.html"))
                }
            }, onDelete = { confirmDelete = e }, onSaveFile = { addr, name ->
                scope.launch {
                    withContext(Dispatchers.IO) { runCatching { Releases.add(context, addr, name) } }
                    message = context.getString(R.string.feed_saved_to_files, name)
                }
            })
        }
    }
}

@Composable
private fun FeedCard(e: FeedEntry, mine: Boolean, onOpen: () -> Unit, onDelete: () -> Unit, onSaveFile: (String, String) -> Unit) {
    val context = LocalContext.current
    Card(Modifier.fillMaxWidth().padding(bottom = 10.dp)) {
        Column(Modifier.padding(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(
                    Modifier.size(36.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primaryContainer),
                    contentAlignment = Alignment.Center,
                ) { Text((e.name.ifBlank { "?" }).take(1).uppercase(), style = MaterialTheme.typography.titleMedium) }
                Spacer(Modifier.width(10.dp))
                Column(Modifier.weight(1f)) {
                    Text(if (mine) stringResource(R.string.feed_name_you, e.name) else e.name, style = MaterialTheme.typography.titleSmall)
                    Text(
                        DateUtils.getRelativeTimeSpanString(e.post.at.toLong() * 1000).toString() + (e.post.edited?.let { " · " + stringResource(R.string.feed_edited) } ?: ""),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                IconButton(onClick = onOpen) { Icon(Icons.Filled.OpenInNew, stringResource(R.string.feed_open_post)) }
                if (mine) IconButton(onClick = onDelete) { Icon(Icons.Filled.Delete, stringResource(R.string.feed_delete_post)) }
            }
            val blocks = remember(e.post.text) { runCatching { uniffi.ducat_mobile.feedBlocks(e.post.text) }.getOrDefault(emptyList()) }
            FeedBlocks(blocks) { path -> HomeImage(e.persona, path, onOpen) }
            for (m in e.post.media) HomeImage(e.persona, m.path, onOpen)
            if (e.post.files.isNotEmpty()) {
                Row(Modifier.padding(top = 6.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    for (f in e.post.files) {
                        AssistChip(onClick = { onSaveFile(f.addr, f.name) }, label = { Text("${f.name} · ${android.text.format.Formatter.formatShortFileSize(context, f.bytes.toLong())}") })
                    }
                }
            }
        }
    }
}

@Composable
private fun HomeImage(persona: String, path: String, onOpen: () -> Unit) {
    val context = LocalContext.current
    val bmp = remember(persona, path) {
        runCatching { Home.homeFile(context, persona, path)?.let { BitmapFactory.decodeFile(it.path) } }.getOrNull()
    }
    if (bmp != null) {
        Image(
            bmp.asImageBitmap(),
            contentDescription = null,
            modifier = Modifier.padding(top = 8.dp).fillMaxWidth().clip(MaterialTheme.shapes.small).clickable { onOpen() },
        )
    }
}
