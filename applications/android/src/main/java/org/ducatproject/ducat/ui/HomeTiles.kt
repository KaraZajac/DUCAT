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
 * The three things a person comes to the personal screen to *start*.
 *
 * They used to be a wide card and a pair of chips under a "Renting nearby"
 * heading, which read as two unrelated features stacked on top of each other
 * and pushed everything below the fold. They are one kind of thing — a moment
 * you begin, not a job you run — so they get one row and the same shape.
 *
 * The labels say "rent" rather than "find" because the grouping heading is
 * gone: beside "Hail a ride", a tile reading "Find a car" invites the reading
 * that it is another way to be driven somewhere.
 */
@Composable
fun HomeTiles(
    onHail: () -> Unit,
    onRentCar: () -> Unit,
    onRentPlace: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier.fillMaxWidth().padding(horizontal = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Tile(
            icon = Icons.Filled.LocalTaxi,
            label = stringResource(R.string.hail_card_title),
            onClick = onHail,
            modifier = Modifier.weight(1f),
        )
        Tile(
            icon = Icons.Filled.DirectionsCar,
            label = stringResource(R.string.home_tile_rent_car),
            onClick = onRentCar,
            modifier = Modifier.weight(1f),
        )
        Tile(
            icon = Icons.Filled.House,
            label = stringResource(R.string.home_tile_rent_place),
            onClick = onRentPlace,
            modifier = Modifier.weight(1f),
        )
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
