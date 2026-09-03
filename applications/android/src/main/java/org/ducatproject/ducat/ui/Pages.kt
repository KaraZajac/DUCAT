package org.ducatproject.ducat.ui

import androidx.compose.foundation.background
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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Public
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.R
import org.ducatproject.ducat.Sites

/**
 * The Pages face (§16.22): the address of the page this phone puts on
 * the network, big enough to scan across a counter.
 *
 * Press's twin, and the difference between them is worth stating because
 * it decides what this screen has to say. A publication is *delivered* —
 * the issue lands in the threads of the people who paid for it, and once
 * it has, it is theirs whatever the publisher does next. A page is
 * *left somewhere*: it exists only while somebody serves it, and until a
 * reader chooses to mirror it, that somebody is this phone. So the
 * status line here is not decoration. "Published" and "reachable" are
 * different facts, and a publisher who closes the mode is allowed to
 * know which one they have.
 */
@Composable
fun PagesScreen() {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val mine = remember(version) { Sites.all(context).filter { it.mine } }

    if (mine.isEmpty()) {
        Column(
            Modifier.fillMaxSize().padding(32.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Box(
                Modifier.size(72.dp).clip(CircleShape)
                    .background(MaterialTheme.colorScheme.surfaceVariant),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    Icons.Filled.Public,
                    contentDescription = null,
                    modifier = Modifier.size(36.dp),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.height(16.dp))
            Text(
                stringResource(R.string.pages_none_title),
                style = MaterialTheme.typography.titleLarge,
            )
            Spacer(Modifier.height(8.dp))
            Text(
                stringResource(R.string.pages_none_body),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )
            Spacer(Modifier.height(20.dp))
            // Making one lives in the room, in one place, exactly as
            // creating a publication does.
            Button(onClick = { shellTabRequest.value = 1 }) {
                Text(stringResource(R.string.pages_open_room))
            }
        }
        return
    }

    val site = mine.first()
    val uri = Sites.uriOf(site.recordKey)

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(site.title, style = MaterialTheme.typography.headlineSmall)
        Spacer(Modifier.height(4.dp))
        Text(
            stringResource(R.string.pages_address_hint),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.height(16.dp))
        QrBlock(uri)
        Spacer(Modifier.height(16.dp))

        // Whether the page can actually be read right now, said plainly.
        // A site is only as reachable as its seeders, and this phone is
        // the only one until a reader ticks keep-alive on it.
        Card(
            Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
            ),
        ) {
            Column(Modifier.padding(14.dp)) {
                Text(
                    stringResource(R.string.pages_serving_title),
                    style = MaterialTheme.typography.titleSmall,
                )
                Spacer(Modifier.height(4.dp))
                Text(
                    stringResource(R.string.pages_serving_body),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        Spacer(Modifier.height(12.dp))

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(
                onClick = { copyText(context, uri, context.getString(R.string.pages_address_copied)) },
                modifier = Modifier.weight(1f),
            ) { Text(stringResource(R.string.pages_copy_address)) }
            Button(
                onClick = { siteOpen(context, site.recordKey) },
                modifier = Modifier.weight(1f),
            ) { Text(stringResource(R.string.pages_preview)) }
        }
        Spacer(Modifier.height(8.dp))
        OutlinedButton(
            onClick = { shellTabRequest.value = 1 },
            modifier = Modifier.fillMaxWidth(),
        ) { Text(stringResource(R.string.pages_edit)) }

        if (mine.size > 1) {
            Spacer(Modifier.height(16.dp))
            Text(
                stringResource(R.string.pages_more, mine.size - 1),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
