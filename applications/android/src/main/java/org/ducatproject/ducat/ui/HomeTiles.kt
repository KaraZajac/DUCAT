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
 * **Six, in the order somebody would think of them**, and each one named as
 * the errand it is rather than as the category it filters. The first version
 * of this row reused the search screen\'s own chips — Places, Cars, Gear, For
 * sale, Skills — which cost no new strings and read like a filing system.
 * "Cars" is a heading; "Rent a car" is a thing you were about to do. The
 * marketplace and hiring somebody are not filters over a board at all in
 * anyone\'s head, whatever they are underneath.
 *
 * Ordered by hand, not by [org.ducatproject.ducat.Listings.KINDS], because
 * that order is a wire numbering and this one is about what belongs beside
 * what: the three rentals together, then buying, then people.
 */
@Composable
fun HomeTiles(
    onHail: () -> Unit,
    /** Open the local board on one of [org.ducatproject.ducat.Listings.KINDS]. */
    onBrowse: (Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    val kinds = org.ducatproject.ducat.Listings
    // Two rows of three rather than a wrapping flow: six tiles across a phone
    // is three and three, and an explicit pair of rows keeps every tile the
    // same width whatever the label does in a language with longer words.
    val tiles = listOf(
        Triple(Icons.Filled.LocalTaxi, R.string.hail_card_title, null),
        Triple(listingIcon(kinds.KIND_VEHICLE), R.string.home_tile_rent_car, kinds.KIND_VEHICLE),
        Triple(listingIcon(kinds.KIND_PLACE), R.string.home_tile_rent_place, kinds.KIND_PLACE),
        Triple(listingIcon(kinds.KIND_GEAR), R.string.home_tile_rent_gear, kinds.KIND_GEAR),
        Triple(listingIcon(kinds.KIND_SALE), R.string.home_tile_marketplace, kinds.KIND_SALE),
        Triple(listingIcon(kinds.KIND_SKILL), R.string.home_tile_hire_help, kinds.KIND_SKILL),
    )
    Column(
        modifier.fillMaxWidth().padding(horizontal = 16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        tiles.chunked(3).forEach { row ->
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                row.forEach { (icon, label, kind) ->
                    Tile(
                        icon = icon,
                        label = stringResource(label),
                        onClick = { if (kind == null) onHail() else onBrowse(kind) },
                        modifier = Modifier.weight(1f),
                    )
                }
                // Keeps a short last row's tiles the same width as a full one.
                repeat(3 - row.size) { Spacer(Modifier.weight(1f)) }
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
