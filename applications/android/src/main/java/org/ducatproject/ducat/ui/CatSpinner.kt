package org.ducatproject.ducat.ui

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathFillType
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Fill
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.vector.PathParser

/**
 * The wait, wearing the brand.
 *
 * A board sweep is most of a minute of nothing happening on screen, and a
 * stock spinner says "computer busy" in everyone's voice at once. This is
 * the same wait in DUCAT's: the mono cat — the one drawn for small sizes,
 * ears and raised paw and the coin punched out of the belly — sitting
 * still while the splash's ring runs around it. The cat does not spin;
 * a cat would not.
 *
 * The path is [ic_cat_mono]'s, inlined as path data rather than read as a
 * resource, because this file compiles on the desk too and the desk's
 * resource table carries rasters only. One string, parsed once per
 * composition, scaled to the box at draw time.
 */
private const val CAT_PATH =
    "M35,32 L30,9 L51,21 C52,20 59,20 60,21 L81,9 L76,32 C80,36 82,41 82,46 " +
        "C82,50 80,54 77,57 C84,61 88,69 88,78 C88,86 83,92 75,92 L37,92 " +
        "C29,92 24,86 24,78 C24,70 27,63 32,58 C29,55 27,51 27,46 " +
        "C27,41 30,36 35,32 Z " +
        "M16,27 C21,27 24,32 24,38 L24,52 C24,57 20,60 16,60 " +
        "C11,60 8,56 8,51 L8,37 C8,31 11,27 16,27 Z " +
        "M56,62 C64,62 71,69 71,77 C71,85 64,92 56,92 " +
        "C48,92 41,85 41,77 C41,69 48,62 56,62 Z"

/** The vector's own canvas — the path above speaks in these units. */
private const val CAT_VIEWPORT = 108f

@Composable
fun CatSpinner(modifier: Modifier = Modifier, tint: Color) {
    val cat = remember {
        Path().apply {
            fillType = PathFillType.EvenOdd
            PathParser().parsePathString(CAT_PATH).toPath(this)
        }
    }
    val angle by rememberInfiniteTransition(label = "catspin").animateFloat(
        initialValue = 0f,
        targetValue = 360f,
        animationSpec = infiniteRepeatable(tween(1100, easing = LinearEasing)),
        label = "angle",
    )
    Canvas(modifier) {
        val stroke = Stroke(width = size.minDimension * 0.09f, cap = StrokeCap.Round)
        drawArc(
            color = tint,
            startAngle = angle,
            sweepAngle = 265f,
            useCenter = false,
            topLeft = Offset(stroke.width, stroke.width),
            size = Size(size.width - 2 * stroke.width, size.height - 2 * stroke.width),
            style = stroke,
        )
        // The cat fills about 55% of the box, centred — clear of the ring the
        // way the launcher icon sits clear of its mask.
        val scale = (size.minDimension * 0.55f) / CAT_VIEWPORT
        val inset = (size.minDimension - CAT_VIEWPORT * scale) / 2f
        translate(inset, inset) {
            scale(scale, scale, pivot = Offset.Zero) {
                drawPath(cat, tint, style = Fill)
            }
        }
    }
}

