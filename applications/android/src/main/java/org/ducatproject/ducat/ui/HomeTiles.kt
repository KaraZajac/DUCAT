package org.ducatproject.ducat.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.DirectionsCar
import androidx.compose.material.icons.filled.House
import androidx.compose.material.icons.filled.LocalTaxi
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.R

/**
 * The things a person comes to the personal screen to *start*.
 *
 * They used to be a wide card and a pair of chips under a "Renting nearby"
 * heading, which read as two unrelated features stacked on top of each other
 * and pushed everything below the fold. They are one kind of thing — a moment
 * you begin, not a job you run — so they get one shape.
 *
 * **All five nouns, not two.** The board grew from rooms and cars to gear,
 * things for sale and people\'s time, and this row did not: somebody could
 * post an electrician\'s hours and nobody had any way to go looking for one.
 * The search screen behind these has handled all five the whole time, with a
 * chip each and a count — the only thing missing was a door into it. So the
 * tiles wear the same words as the chips they land on, which also means they
 * cost no new strings in nineteen languages.
 */
@Composable
fun HomeTiles(
    onHail: () -> Unit,
    /** Open the local board on one of [org.ducatproject.ducat.Listings.KINDS]. */
    onBrowse: (Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    // Two rows of three rather than a wrapping flow: six tiles across a phone
    // is three and three, and an explicit pair of rows keeps every tile the
    // same width whatever the label does in a language with longer words.
    val kinds = org.ducatproject.ducat.Listings.KINDS
    Column(
        modifier.fillMaxWidth().padding(horizontal = 16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Tile(
                icon = Icons.Filled.LocalTaxi,
                label = stringResource(R.string.hail_card_title),
                onClick = onHail,
                modifier = Modifier.weight(1f),
            )
            kinds.take(2).forEach { k ->
                Tile(
                    icon = listingIcon(k),
                    label = stringResource(boardChipLabel(k)),
                    onClick = { onBrowse(k) },
                    modifier = Modifier.weight(1f),
                )
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            kinds.drop(2).forEach { k ->
                Tile(
                    icon = listingIcon(k),
                    label = stringResource(boardChipLabel(k)),
                    onClick = { onBrowse(k) },
                    modifier = Modifier.weight(1f),
                )
            }
            // Keeps the second row's tiles the same width as the first when
            // the kinds do not divide by three.
            repeat(3 - (kinds.size - 2).coerceAtMost(3)) {
                Spacer(Modifier.weight(1f))
            }
        }
    }
}

/**
 * One square, sized by the row rather than by a fixed height: three of them
 * across a narrow phone are about a hundred dp, and a label that wraps to two
 * lines on a long word must not clip against a hardcoded box.
 */
@Composable
private fun Tile(
    icon: ImageVector,
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceVariant,
        shape = MaterialTheme.shapes.large,
        modifier = modifier.aspectRatio(1f).clickable(onClick = onClick),
    ) {
        Column(
            Modifier.fillMaxSize().padding(10.dp),
            verticalArrangement = Arrangement.Center,
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Icon(icon, null, Modifier.size(28.dp), tint = MaterialTheme.colorScheme.primary)
            Spacer(Modifier.height(8.dp))
            Text(
                label,
                style = MaterialTheme.typography.labelLarge,
                textAlign = TextAlign.Center,
                maxLines = 2,
            )
        }
    }
}
