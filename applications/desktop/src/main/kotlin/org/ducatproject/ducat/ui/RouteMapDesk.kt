package org.ducatproject.ducat.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.dp

/**
 * The route, drawn rather than tiled.
 *
 * The phone's map is osmdroid — an Android view, and the one dependency in
 * this app that cannot cross to a JVM. Rather than leave a hole where the
 * route belongs, the desk draws the same three facts with the same
 * geometry: where the ride starts, where it ends, and the shape of the road
 * between them. What it deliberately does *not* draw is the world underneath.
 *
 * That is a smaller picture, and also the more private one. §15.12 already
 * names OpenStreetMap's tile servers as the single place this app sends
 * location off-device; a desk that draws only the route sends nothing at
 * all, and the route it draws came from the same query the fare did.
 */
@Composable
fun RouteMap(
    from: Pair<Long, Long>?,
    to: Pair<Long, Long>?,
    route: List<Pair<Long, Long>>,
    modifier: Modifier = Modifier,
) {
    val points = buildList {
        from?.let { add(it) }
        addAll(route)
        to?.let { add(it) }
    }
    val line = MaterialTheme.colorScheme.primary
    val startColour = MaterialTheme.colorScheme.tertiary
    val endColour = MaterialTheme.colorScheme.error
    val faint = MaterialTheme.colorScheme.surfaceVariant

    Box(modifier, contentAlignment = Alignment.Center) {
        if (points.size < 2) {
            Text(
                "No route yet",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            return@Box
        }
        Canvas(Modifier.fillMaxSize().padding(12.dp)) {
            val lats = points.map { it.first }
            val lons = points.map { it.second }
            val minLat = lats.min().toDouble()
            val maxLat = lats.max().toDouble()
            val minLon = lons.min().toDouble()
            val maxLon = lons.max().toDouble()
            // Equal-aspect fit: a route squeezed to fill a box is a route
            // whose shape lies about the turn it describes.
            val spanLat = (maxLat - minLat).coerceAtLeast(1.0)
            val spanLon = (maxLon - minLon).coerceAtLeast(1.0)
            val scale = minOf(size.width / spanLon, size.height / spanLat)
            val offX = (size.width - spanLon * scale) / 2
            val offY = (size.height - spanLat * scale) / 2

            // Latitude grows north; a canvas grows down.
            fun at(p: Pair<Long, Long>): Offset {
                val px = (offX + (p.second - minLon) * scale).toFloat()
                val py = (offY + (maxLat - p.first) * scale).toFloat()
                return androidx.compose.ui.geometry.Offset(px, py)
            }

            drawRect(color = faint.copy(alpha = 0.25f), size = size)
            val path = Path().apply {
                val first = at(points.first())
                moveTo(first.x, first.y)
                points.drop(1).forEach { p -> at(p).let { lineTo(it.x, it.y) } }
            }
            drawPath(path, color = line, style = Stroke(width = 4f))
            from?.let { drawCircle(startColour, radius = 7f, center = at(it)) }
            to?.let { drawCircle(endColour, radius = 7f, center = at(it)) }
        }
    }
}

/**
 * Search results, drawn the same way: where each candidate is relative to
 * here, and to each other. Tapping one picks it, as on the phone.
 *
 * Without tiles this is a constellation rather than a map — it says which
 * results are clustered and which is off on its own, and it cannot say which
 * side of the river any of them is on. That is the same trade the rest of
 * this file makes and for the same reason: the desk draws what it was already
 * told and asks nobody for the world underneath. The distances in the list
 * beside it are the precise half of the answer.
 */
@Composable
fun ResultsMap(
    me: Pair<Long, Long>?,
    results: List<Pair<Pair<Long, Long>, String>>,
    onPick: (Int) -> Unit,
    modifier: Modifier = Modifier,
) = DriverMap(
    me = me,
    fares = results,
    onFareTap = onPick,
    coverage = null,
    modifier = modifier,
)

/**
 * The driver's net, drawn the same way: the desk's own position, the fares
 * on the boards it watches, and the outer bounds of that watch. Tapping a
 * fare works here as it does on the phone — the marker is the button.
 */
@Composable
fun DriverMap(
    me: Pair<Long, Long>?,
    fares: List<Pair<Pair<Long, Long>, String>>,
    onFareTap: (Int) -> Unit,
    coverage: LongArray? = null,
    modifier: Modifier = Modifier,
) {
    val meColour = MaterialTheme.colorScheme.tertiary
    val fareColour = MaterialTheme.colorScheme.primary
    val netColour = MaterialTheme.colorScheme.outline
    val faint = MaterialTheme.colorScheme.surfaceVariant
    // Every point that must fit: the desk, the fares, and the watched net.
    val pts = buildList {
        me?.let { add(it) }
        fares.forEach { add(it.first) }
        coverage?.takeIf { it.size >= 4 }?.let {
            add(it[0] to it[2]); add(it[1] to it[3])
        }
    }
    Box(modifier, contentAlignment = Alignment.Center) {
        if (pts.isEmpty()) {
            Text(
                "Watching the boards — nothing on them yet",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            return@Box
        }
        var spots by remember { mutableStateOf<List<Pair<Offset, Int>>>(emptyList()) }
        Canvas(
            Modifier.fillMaxSize().padding(12.dp)
                .then(
                    tapAnywhere { tap ->
                        spots.minByOrNull { (o, _) ->
                            (o.x - tap.x) * (o.x - tap.x) + (o.y - tap.y) * (o.y - tap.y)
                        }?.takeIf { (o, _) ->
                            val dx = o.x - tap.x
                            val dy = o.y - tap.y
                            dx * dx + dy * dy < 30f * 30f
                        }?.let { (_, i) -> onFareTap(i) }
                    },
                ),
        ) {
            val lats = pts.map { it.first }
            val lons = pts.map { it.second }
            val minLat = lats.min().toDouble()
            val maxLat = lats.max().toDouble()
            val minLon = lons.min().toDouble()
            val maxLon = lons.max().toDouble()
            val spanLat = (maxLat - minLat).coerceAtLeast(1.0)
            val spanLon = (maxLon - minLon).coerceAtLeast(1.0)
            val scale = minOf(size.width / spanLon, size.height / spanLat)
            val offX = (size.width - spanLon * scale) / 2
            val offY = (size.height - spanLat * scale) / 2
            fun at(p: Pair<Long, Long>): Offset {
                val px = (offX + (p.second - minLon) * scale).toFloat()
                val py = (offY + (maxLat - p.first) * scale).toFloat()
                return androidx.compose.ui.geometry.Offset(px, py)
            }

            drawRect(color = faint.copy(alpha = 0.25f), size = size)
            coverage?.takeIf { it.size >= 4 }?.let { c ->
                val a = at(c[1] to c[2])
                val b = at(c[0] to c[3])
                drawRect(
                    color = netColour,
                    topLeft = androidx.compose.ui.geometry.Offset(minOf(a.x, b.x), minOf(a.y, b.y)),
                    size = androidx.compose.ui.geometry.Size(
                        kotlin.math.abs(b.x - a.x), kotlin.math.abs(b.y - a.y),
                    ),
                    style = Stroke(width = 2f),
                )
            }
            val placed = mutableListOf<Pair<Offset, Int>>()
            fares.forEachIndexed { i, (p, _) ->
                val o = at(p)
                placed += o to i
                drawCircle(fareColour, radius = 9f, center = o)
            }
            spots = placed
            me?.let { drawCircle(meColour, radius = 7f, center = at(it)) }
        }
    }
}

/** "Tap anywhere on this canvas", as a modifier. */
private fun tapAnywhere(onTap: (Offset) -> Unit): Modifier =
    Modifier.pointerInput(Unit) { detectTapGestures(onTap = onTap) }
