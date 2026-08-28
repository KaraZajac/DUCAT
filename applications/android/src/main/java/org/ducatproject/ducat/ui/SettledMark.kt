package org.ducatproject.ducat.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke

/**
 * The settled moment, drawn rather than stamped.
 *
 * An escrow ending well is the one instant somebody actually watches this
 * screen — money they were owed just arrived — and a static lock icon gave
 * it the same weight as "waiting". This draws a ring closing and then a
 * check striking through it, once, on first appearance. After the draw it
 * is simply a checkmark: the animation is an arrival, not a decoration
 * that loops while someone tries to read the amount under it.
 *
 * One-shot by construction: the two Animatables live in the composition,
 * so the draw replays only when the settled banner itself is born again —
 * a fresh ceremony, not a recomposition or a scroll.
 */
@Composable
fun SettledMark(modifier: Modifier = Modifier, tint: Color) {
    val ring = remember { Animatable(0f) }
    val check = remember { Animatable(0f) }
    LaunchedEffect(Unit) {
        ring.animateTo(1f, tween(durationMillis = 450))
        check.animateTo(1f, tween(durationMillis = 250))
    }
    Canvas(modifier) {
        val stroke = Stroke(width = size.minDimension * 0.11f, cap = StrokeCap.Round)
        // The ring closes clockwise from the top.
        drawArc(
            color = tint,
            startAngle = -90f,
            sweepAngle = 360f * ring.value,
            useCenter = false,
            topLeft = Offset(stroke.width, stroke.width),
            size = Size(size.width - 2 * stroke.width, size.height - 2 * stroke.width),
            style = stroke,
        )
        // The check: a short drop then the long rise, drawn as one stroke.
        // Fractions of the box, eyeballed against material's own check.
        val a = Offset(size.width * 0.28f, size.height * 0.53f)
        val b = Offset(size.width * 0.44f, size.height * 0.68f)
        val c = Offset(size.width * 0.72f, size.height * 0.34f)
        val t = check.value
        if (t > 0f) {
            // First 40% of the draw is the drop, the rest the rise.
            val drop = (t / 0.4f).coerceAtMost(1f)
            drawLine(tint, a, Offset(a.x + (b.x - a.x) * drop, a.y + (b.y - a.y) * drop), stroke.width, StrokeCap.Round)
            if (t > 0.4f) {
                val rise = ((t - 0.4f) / 0.6f).coerceAtMost(1f)
                drawLine(tint, b, Offset(b.x + (c.x - b.x) * rise, b.y + (c.y - b.y) * rise), stroke.width, StrokeCap.Round)
            }
        }
    }
}
