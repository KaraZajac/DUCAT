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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.horizontalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
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

/**
 * The worldwide shelf (§16.18.2): category boards instead of geohash
 * cells, publications instead of kayaks, and Subscribe instead of a
 * conversation about pick-up. Subscribing IS the ordinary card claim —
 * the row hands its `ducat:` card to the same confirm sheet a scanned
 * code opens, and §16.20 does the rest.
 */
@Composable
fun WorldwideMarket() {
    val context = LocalContext.current
    var cat by rememberSaveable { mutableStateOf("news") }
    // The phone's language first: a worldwide board that opens on a wall
    // of elsewhere is nobody's shelf. "All languages" is one tap away.
    var myLang by rememberSaveable { mutableStateOf(true) }
    val lang = java.util.Locale.getDefault().language.takeIf { it.isNotBlank() }
    var rows by remember { mutableStateOf<List<Publications.MarketRow>>(emptyList()) }
    var looked by remember { mutableStateOf(false) }
    LaunchedEffect(cat, myLang) {
        looked = false
        rows = withContext(Dispatchers.IO) {
            runCatching {
                Publications.browseMarket(
                    context, cat, if (myLang) lang else null,
                )
            }.getOrDefault(emptyList())
        }
        looked = true
    }
    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())
                .padding(horizontal = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Publications.MARKET_CATEGORIES.forEach { slug ->
                FilterChip(
                    selected = cat == slug,
                    onClick = { cat = slug },
                    label = { Text(marketCategoryLabel(slug)) },
                )
            }
        }
        if (lang != null) {
            Row(Modifier.padding(horizontal = 16.dp)) {
                FilterChip(
                    selected = myLang,
                    onClick = { myLang = !myLang },
                    label = {
                        Text(
                            if (myLang) lang else stringResource(R.string.market_all_langs),
                            style = MaterialTheme.typography.labelSmall,
                        )
                    },
                )
            }
        }
        Spacer(Modifier.height(4.dp))
        if (looked && rows.isEmpty()) {
            Text(
                stringResource(R.string.market_empty),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(24.dp),
            )
        }
        LazyColumn(Modifier.fillMaxSize()) {
            items(rows.size) { i ->
                val r = rows[i]
                Surface(
                    shape = MaterialTheme.shapes.medium,
                    color = MaterialTheme.colorScheme.surfaceVariant,
                    modifier = Modifier.fillMaxWidth()
                        .padding(horizontal = 16.dp, vertical = 4.dp),
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
                        Row(
                            verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
                        ) {
                            Text(
                                r.pricePxmr?.let {
                                    stringResource(
                                        R.string.market_per_period,
                                        Amounts.show(context, it).primary,
                                    )
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
        }
    }
}
