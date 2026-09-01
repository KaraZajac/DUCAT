package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.Publications
import org.ducatproject.ducat.R

/**
 * How a Subscribe tap hands its card off. The phone routes it into the
 * ordinary claim sheet (MainActivity wires this at launch); the desk's
 * rooms wire their own. Shared sources cannot name an Activity.
 */
var marketSubscribe: (String) -> Unit = {}

/** And how "list yours" reaches the Publishing room, same reason. */
var marketListYours: () -> Unit = {}

/** A category slug's human name. */
@Composable
internal fun marketCategoryLabel(slug: String): String = stringResource(
    when (slug) {
        "news" -> R.string.market_cat_news
        "serials" -> R.string.market_cat_serials
        "sound" -> R.string.market_cat_sound
        "software" -> R.string.market_cat_software
        "art" -> R.string.market_cat_art
        else -> R.string.market_cat_other
    },
)

/** One publication row, wherever its board was. */
@Composable
private fun MarketRowCard(r: Publications.MarketRow) {
    val context = LocalContext.current
    Surface(
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.surfaceVariant,
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
    ) {
        Column(Modifier.padding(12.dp)) {
            Text(r.title, style = MaterialTheme.typography.titleSmall)
            r.blurb?.let {
                Text(
                    it, style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.height(6.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    r.pricePxmr?.let {
                        stringResource(R.string.market_per_period, Amounts.show(context, it).primary)
                    } ?: stringResource(R.string.market_free),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.ducat.settled,
                )
                Spacer(Modifier.weight(1f))
                Button(onClick = { marketSubscribe(r.cardUri) }) {
                    Text(stringResource(R.string.market_subscribe))
                }
            }
        }
    }
}

/** Shared tail: the looking line, the empty state with its invitation,
 *  and the rows. */
@Composable
@androidx.compose.material3.ExperimentalMaterial3Api
private fun ShelfBody(
    rows: List<Publications.MarketRow>,
    looked: Boolean,
    looking: String,
    refreshing: Boolean = false,
    onRefresh: (() -> Unit)? = null,
) {
    val context = LocalContext.current
    // One column of our own: the callers place this body in containers
    // that stack children on top of each other, and the refresh line drew
    // behind the first card. What reads as rows must be laid out as rows.
    // No early returns inside — a bare return from a Compose lambda has
    // crashed this app before (IntStack.peek2); the branches are an
    // if/else chain instead.
    Column(Modifier.fillMaxSize()) {
        if (refreshing && rows.isNotEmpty()) {
            // Painted from memory while the live read runs: say so, quietly.
            Row(
                Modifier.padding(horizontal = 24.dp, vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                CircularProgressIndicator(
                    Modifier.size(14.dp),
                    strokeWidth = 2.dp,
                )
                Spacer(Modifier.padding(4.dp))
                Text(
                    stringResource(R.string.market_refreshing),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        if (!looked) {
            Row(
                Modifier.padding(horizontal = 24.dp, vertical = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                CircularProgressIndicator(Modifier.height(18.dp))
                Spacer(Modifier.padding(6.dp))
                Text(
                    looking,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else if (rows.isEmpty()) {
            // Scrollable so the pull gesture works from the empty answer —
            // which is exactly where somebody most wants to ask again.
            Column(
                Modifier.fillMaxSize()
                    .let { m ->
                        if (onRefresh != null) {
                            m.verticalScroll(androidx.compose.foundation.rememberScrollState())
                        } else {
                            m
                        }
                    }
                    .padding(24.dp),
            ) {
                Text(
                    stringResource(R.string.market_empty),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                // The invitation, but only for somebody who can accept it.
                val hasPubs = remember { Publications.publications(context).isNotEmpty() }
                if (hasPubs) {
                    Spacer(Modifier.height(8.dp))
                    OutlinedButton(onClick = { marketListYours() }) {
                        Text(stringResource(R.string.market_list_yours))
                    }
                }
            }
        } else {
            LazyColumn(Modifier.fillMaxSize()) {
                items(rows.size) { i -> MarketRowCard(rows[i]) }
            }
        }
    }
}

/** ShelfBody behind a pull: dragging down asks the shelf again. The
 *  stale-while-revalidate rows stay on screen while the fresh read runs,
 *  so a pull is never a blank screen. */
@androidx.compose.material3.ExperimentalMaterial3Api
@Composable
private fun ShelfPull(
    refreshing: Boolean,
    onRefresh: () -> Unit,
    content: @Composable () -> Unit,
) {
    androidx.compose.material3.pulltorefresh.PullToRefreshBox(
        isRefreshing = refreshing,
        onRefresh = onRefresh,
    ) {
        content()
    }
}


/** Wait for the node before asking the network anything: a shelf read
 *  while unattached "succeeds" empty in a blink, and an empty answer
 *  from a device that could not ask is a confident lie — one that used
 *  to overwrite the remembered rows already on screen. */
private suspend fun awaitAttached(maxMs: Long): Boolean {
    val end = System.currentTimeMillis() + maxMs
    while (System.currentTimeMillis() < end) {
        if (runCatching { uniffi.ducat_mobile.nodeStatus().publicInternetReady }
                .getOrDefault(false)
        ) {
            return true
        }
        kotlinx.coroutines.delay(1_500)
    }
    return false
}

/** The worldwide shelf for one category (§16.18.2). */
@Composable
@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
fun WorldwideShelf(cat: String, myLangOnly: Boolean) {
    val context = LocalContext.current
    val lang = java.util.Locale.getDefault().language.takeIf { it.isNotBlank() }
    var rows by remember { mutableStateOf<List<Publications.MarketRow>>(emptyList()) }
    var looked by remember { mutableStateOf(false) }
    var refreshing by remember { mutableStateOf(false) }
    var attempt by remember { mutableStateOf(0) }
    LaunchedEffect(cat, myLangOnly, attempt) {
        looked = false
        val wanted = if (myLangOnly) lang else null
        // What this shelf said last time paints now; the live read replaces
        // it. The remembered choice is also what the background warmer keeps
        // fresh between visits.
        val warm = withContext(Dispatchers.IO) {
            context.getSharedPreferences("ducat_market_cache", 0).edit()
                .putString("last_cat", cat)
                .putString("last_lang", wanted ?: "").apply()
            runCatching { Publications.cachedMarket(context, cat, wanted) }.getOrNull()
        }
        if (!warm.isNullOrEmpty()) {
            rows = warm
            looked = true
            refreshing = true
        }
        if (withContext(Dispatchers.IO) { awaitAttached(120_000) }) {
            val fresh = withContext(Dispatchers.IO) {
                runCatching {
                    Publications.browseMarket(context, cat, wanted)
                }.getOrDefault(emptyList())
            }
            rows = fresh
        }
        refreshing = false
        looked = true
    }
    ShelfPull(refreshing = refreshing, onRefresh = { attempt++ }) {
        ShelfBody(
            rows, looked,
            stringResource(R.string.market_looking, marketCategoryLabel(cat)),
            refreshing = refreshing,
            onRefresh = { attempt++ },
        )
    }
}

/** The local shelf: publications on the neighbourhood's own boards. */
@Composable
@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
fun LocalShelf() {
    val context = LocalContext.current
    var rows by remember { mutableStateOf<List<Publications.MarketRow>>(emptyList()) }
    var looked by remember { mutableStateOf(false) }
    var progress by remember { mutableStateOf(0 to 9) }
    var noFix by remember { mutableStateOf(false) }
    var refreshing by remember { mutableStateOf(false) }
    // Bumped by Try again: a missing fix is often momentary — location
    // just switched on, or the phone found the sky — and a dead end with
    // no door back was the only place in the market you could get stuck.
    var attempt by remember { mutableStateOf(0) }
    LaunchedEffect(attempt) {
        noFix = false
        looked = false
        grabFix(context) { fix ->
            if (fix == null) {
                noFix = true
                looked = true
                return@grabFix
            }
            MainScope().launch(Dispatchers.IO) {
                val warm = runCatching {
                    Publications.cachedLocalPubs(context, fix.first, fix.second)
                }.getOrNull()
                if (!warm.isNullOrEmpty()) {
                    withContext(Dispatchers.Main) {
                        rows = warm
                        looked = true
                        refreshing = true
                    }
                }
                if (awaitAttached(120_000)) {
                    val got = runCatching {
                        Publications.browseLocalPubs(context, fix.first, fix.second) { k, n ->
                            progress = k to n
                        }
                    }.getOrDefault(emptyList())
                    withContext(Dispatchers.Main) { rows = got }
                }
                withContext(Dispatchers.Main) {
                    looked = true
                    refreshing = false
                }
            }
        }
    }
    if (noFix) {
        Column(Modifier.padding(24.dp)) {
            Text(
                stringResource(R.string.market_no_fix),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            OutlinedButton(onClick = { attempt++ }) {
                Text(stringResource(R.string.rent_search_retry))
            }
        }
        return
    }
    ShelfPull(refreshing = refreshing, onRefresh = { attempt++ }) {
        ShelfBody(
            rows, looked,
            stringResource(R.string.market_looking_local, progress.first, progress.second),
            refreshing = refreshing,
            onRefresh = { attempt++ },
        )
    }
}
