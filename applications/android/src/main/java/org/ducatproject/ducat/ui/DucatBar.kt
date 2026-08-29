package org.ducatproject.ducat.ui

import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp

/**
 * The wait, measured — the linear companion to [CatSpinner]'s circle.
 *
 * Two facts shaped it. A bar that jumps from 3/9 to 4/9 in one frame reads
 * as broken twice — once standing still, once teleporting — so the fill
 * animates between values. And a bar that has genuinely stopped moving (a
 * board taking its full twenty-one seconds) is indistinguishable from a
 * hang, so a soft highlight keeps sweeping the filled part: the number is
 * stuck, the app is not.
 *
 * `progress` null means "working, cannot say how far" — the same bar, with
 * a comet sweeping the track instead of a fill. The two states share one
 * composable so a wait that becomes measurable (a search that learns its
 * board count) morphs in place instead of swapping widgets.
 */
@Composable
fun DucatBar(
    progress: Float?,
    modifier: Modifier = Modifier.fillMaxWidth().height(6.dp),
    color: Color = MaterialTheme.colorScheme.primary,
    track: Color = MaterialTheme.colorScheme.surfaceVariant,
) {
    val fill by animateFloatAsState(
        targetValue = (progress ?: 0f).coerceIn(0f, 1f),
        animationSpec = tween(500, easing = FastOutSlowInEasing),
        label = "fill",
    )
    // One clock for both lives: the shimmer over a determinate fill and the
    // comet of an indeterminate sweep.
    val sweep by rememberInfiniteTransition(label = "bar").animateFloat(
        initialValue = 0f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(tween(1400, easing = LinearEasing)),
        label = "sweep",
    )
    // A bar reads in the script's direction: in an RTL locale progress
    // fills from the right, as Material's own indicator does. The whole
    // drawing is mirrored rather than each shape re-derived — the gradients
    // flip with it.
    val rtl = LocalLayoutDirection.current == LayoutDirection.Rtl
    Canvas(modifier) {
        scale(scaleX = if (rtl) -1f else 1f, scaleY = 1f) {
        val r = CornerRadius(size.height / 2f)
        drawRoundRect(color = track, cornerRadius = r)
        if (progress != null) {
            val w = size.width * fill
            if (w > size.height) {
                drawRoundRect(color = color, size = Size(w, size.height), cornerRadius = r)
                // The living part: a soft light passing along what is done.
                // Width and travel are fractions of the *fill*, so a bar at
                // 10% shimmers inside its 10% instead of spilling onto the
                // track and claiming progress that has not happened.
                val glowW = (w * 0.35f).coerceAtLeast(size.height * 2)
                val x = (w + glowW) * sweep - glowW
                drawRoundRect(
                    brush = Brush.horizontalGradient(
                        0f to Color.Transparent,
                        0.5f to Color.White.copy(alpha = 0.25f),
                        1f to Color.Transparent,
                        startX = x,
                        endX = x + glowW,
                    ),
                    size = Size(w, size.height),
                    cornerRadius = r,
                )
            } else if (w > 0f) {
                // Too thin for a shimmer to fit inside; a plain seed dot.
                drawRoundRect(
                    color = color,
                    size = Size(size.height.coerceAtMost(w + size.height / 2), size.height),
                    cornerRadius = r,
                )
            }
        } else {
            // Indeterminate: a comet crossing the track, eased so it lingers
            // mid-flight and clears the ends — the stock Material sweep, in
            // the brand's own colours and geometry.
            val cometW = size.width * 0.30f
            val travel = size.width + cometW
            val eased = FastOutSlowInEasing.transform(sweep)
            val x = travel * eased - cometW
            drawRoundRect(
                brush = Brush.horizontalGradient(
                    0f to Color.Transparent,
                    0.35f to color,
                    1f to color,
                    startX = x,
                    endX = x + cometW,
                ),
                topLeft = Offset(0f, 0f),
                size = Size(size.width, size.height),
                cornerRadius = r,
            )
        }
        }
    }
}
