package org.ducatproject.ducat.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.Swarm

/**
 * A swarm transfer drawn as the thing it actually is.
 *
 * DUCAT fetches like a torrent, not like a download. The lease manager pops
 * the next piece out of a `HashSet` of what is still wanted, filtered by
 * what each particular peer holds, across several pools at once — so pieces
 * land *scattered*, and a left-to-right bar implies an order the swarm does
 * not use. What a reader actually wants to know is not "how far along" but
 * "is anything arriving, and from how much of the file" — and the honest
 * answer to that is a picture.
 *
 * A publication can be hundreds of pieces and a site of a few hundred
 * megabytes is no different, so the cells are *neighbourhoods*, not pieces:
 * with more pieces than cells each cell covers a run of them and is shaded
 * by how much of its run has landed. Nothing is rounded up — a cell is only
 * full when its whole run is — because a bar that reads finished before the
 * file is would be the same lie as "0 B of 1.0 kB" was, pointing the other
 * way.
 *
 * Falls back to an indeterminate bar while the piece count is unknown: a row
 * of empty cells would claim we know the size of something we do not.
 */
@Composable
fun PieceBar(p: Swarm.Progress?, modifier: Modifier = Modifier) {
    val total = p?.piecesTotal?.toInt() ?: 0
    if (p == null || total <= 0) {
        LinearProgressIndicator(
            modifier = modifier.fillMaxWidth().height(6.dp),
            color = MaterialTheme.colorScheme.primary,
            trackColor = MaterialTheme.colorScheme.surfaceVariant,
        )
        return
    }
    val fills = cellFills(p, total)
    val empty = MaterialTheme.colorScheme.surfaceVariant
    val full = MaterialTheme.colorScheme.primary
    Row(
        modifier.fillMaxWidth().height(10.dp),
        horizontalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        for (share in fills) {
            Box(
                Modifier
                    .weight(1f)
                    .fillMaxHeight()
                    .clip(MaterialTheme.shapes.extraSmall)
                    .background(if (share <= 0f) empty else lerp(empty, full, share)),
            )
        }
    }
}

/** How many cells a bar of [total] pieces is drawn with: one each while
 *  they fit on a phone, neighbourhoods past that. */
fun barCells(total: Int): Int = minOf(total, 40)

/**
 * How full each cell is, 0f..1f — the whole of the bar's arithmetic, kept
 * out of the composable so it can be checked.
 *
 * Every piece belongs to exactly one cell: bounds are integer positions of
 * the cell edges, so runs abut without overlapping and nothing is counted
 * twice at a seam. A cell reaches 1f only when its whole run has landed —
 * no rounding up, because a bar that reads finished before the file is
 * would be the same kind of lie the byte count was.
 */
fun cellFills(p: Swarm.Progress, total: Int): FloatArray {
    if (total <= 0) return FloatArray(0)
    val cells = barCells(total)
    return FloatArray(cells) { cell ->
        val from = (cell.toLong() * total / cells).toInt()
        val to = ((cell + 1).toLong() * total / cells).toInt().coerceAtLeast(from + 1)
        val end = minOf(to, total)
        var have = 0
        for (i in from until end) if (p.has(i)) have++
        val run = (end - from).coerceAtLeast(1)
        have.toFloat() / run.toFloat()
    }
}

/** Local alias so this file does not depend on the foundation Box import
 *  order used elsewhere in the package. */
@Composable
private fun Box(modifier: Modifier) {
    Spacer(modifier)
}
