package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
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
private fun ShelfBody(
    rows: List<Publications.MarketRow>,
    looked: Boolean,
    looking: String,
) {
    val context = LocalContext.current
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
        return
    }
    if (rows.isEmpty()) {
        Column(Modifier.padding(24.dp)) {
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
        return
    }
    LazyColumn(Modifier.fillMaxSize()) {
        items(rows.size) { i -> MarketRowCard(rows[i]) }
    }
}

/** The worldwide shelf for one category (§16.18.2). */
@Composable
fun WorldwideShelf(cat: String, myLangOnly: Boolean) {
    val context = LocalContext.current
    val lang = java.util.Locale.getDefault().language.takeIf { it.isNotBlank() }
    var rows by remember { mutableStateOf<List<Publications.MarketRow>>(emptyList()) }
    var looked by remember { mutableStateOf(false) }
    LaunchedEffect(cat, myLangOnly) {
        looked = false
        rows = withContext(Dispatchers.IO) {
            runCatching {
                Publications.browseMarket(context, cat, if (myLangOnly) lang else null)
            }.getOrDefault(emptyList())
        }
        looked = true
    }
    ShelfBody(
        rows, looked,
        stringResource(R.string.market_looking, marketCategoryLabel(cat)),
    )
}

/** The local shelf: publications on the neighbourhood's own boards. */
@Composable
fun LocalShelf() {
    val context = LocalContext.current
    var rows by remember { mutableStateOf<List<Publications.MarketRow>>(emptyList()) }
    var looked by remember { mutableStateOf(false) }
    var progress by remember { mutableStateOf(0 to 9) }
    var noFix by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        grabFix(context) { fix ->
            if (fix == null) {
                noFix = true
                looked = true
                return@grabFix
            }
            MainScope().launch(Dispatchers.IO) {
                val got = runCatching {
                    Publications.browseLocalPubs(context, fix.first, fix.second) { k, n ->
                        progress = k to n
                    }
                }.getOrDefault(emptyList())
                withContext(Dispatchers.Main) {
                    rows = got
                    looked = true
                }
            }
        }
    }
    if (noFix) {
        Text(
            stringResource(R.string.market_no_fix),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(24.dp),
        )
        return
    }
    ShelfBody(
        rows, looked,
        stringResource(R.string.market_looking_local, progress.first, progress.second),
    )
}
