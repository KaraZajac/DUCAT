package org.ducatproject.ducat.ui

import androidx.compose.ui.graphics.Color

/**
 * Catppuccin — Latte for light, Mocha for dark.
 *
 * Both palettes are defined in full rather than only the shades currently used.
 * A palette that grows a colour at a time drifts into being an approximation of
 * itself, and the point of adopting a published one is that somebody has already
 * balanced it.
 *
 * Accent is **mauve**, Catppuccin's own default. Yellow is reserved for the one
 * thing it must mean here — change coming back — because a colour that carries a
 * meaning cannot also be decoration.
 */
object Latte {
    val rosewater = Color(0xFFDC8A78); val flamingo = Color(0xFFDD7878)
    val pink = Color(0xFFEA76CB);      val mauve = Color(0xFF8839EF)
    val red = Color(0xFFD20F39);       val maroon = Color(0xFFE64553)
    val peach = Color(0xFFFE640B);     val yellow = Color(0xFFDF8E1D)
    val green = Color(0xFF40A02B);     val teal = Color(0xFF179299)
    val sky = Color(0xFF04A5E5);       val sapphire = Color(0xFF209FB5)
    val blue = Color(0xFF1E66F5);      val lavender = Color(0xFF7287FD)
    val text = Color(0xFF4C4F69);      val subtext1 = Color(0xFF5C5F77)
    val subtext0 = Color(0xFF6C6F85);  val overlay2 = Color(0xFF7C7F93)
    val overlay1 = Color(0xFF8C8FA1);  val overlay0 = Color(0xFF9CA0B0)
    val surface2 = Color(0xFFACB0BE);  val surface1 = Color(0xFFBCC0CC)
    val surface0 = Color(0xFFCCD0DA);  val base = Color(0xFFEFF1F5)
    val mantle = Color(0xFFE6E9EF);    val crust = Color(0xFFDCE0E8)
}

object Mocha {
    val rosewater = Color(0xFFF5E0DC); val flamingo = Color(0xFFF2CDCD)
    val pink = Color(0xFFF5C2E7);      val mauve = Color(0xFFCBA6F7)
    val red = Color(0xFFF38BA8);       val maroon = Color(0xFFEBA0AC)
    val peach = Color(0xFFFAB387);     val yellow = Color(0xFFF9E2AF)
    val green = Color(0xFFA6E3A1);     val teal = Color(0xFF94E2D5)
    val sky = Color(0xFF89DCEB);       val sapphire = Color(0xFF74C7EC)
    val blue = Color(0xFF89B4FA);      val lavender = Color(0xFFB4BEFE)
    val text = Color(0xFFCDD6F4);      val subtext1 = Color(0xFFBAC2DE)
    val subtext0 = Color(0xFFA6ADC8);  val overlay2 = Color(0xFF9399B2)
    val overlay1 = Color(0xFF7F849C);  val overlay0 = Color(0xFF6C7086)
    val surface2 = Color(0xFF585B70);  val surface1 = Color(0xFF45475A)
    val surface0 = Color(0xFF313244);  val base = Color(0xFF1E1E2E)
    val mantle = Color(0xFF181825);    val crust = Color(0xFF11111B)
}

/**
 * Colours that carry meaning rather than style.
 *
 * Kept apart from Material's scheme on purpose: `error` in a colour scheme is a
 * UI state, whereas these are protocol facts. Change coming back is not a
 * warning, and a float that cannot pay is not a decoration.
 */
data class DucatColors(
    /** §17.2's locked change — a consequence of having spent, not a fault. */
    val changePending: Color,
    /** §17.2's float running out, which must be said before the counter. */
    val lowCapacity: Color,
    /** Settled, co-signed, done. */
    val settled: Color,
    /** A refusal: a bad signature, a stale bond, an untrusted arbiter. */
    val refused: Color,
)

val LatteMeaning = DucatColors(
    changePending = Latte.yellow,
    lowCapacity = Latte.peach,
    settled = Latte.green,
    refused = Latte.red,
)

val MochaMeaning = DucatColors(
    changePending = Mocha.yellow,
    lowCapacity = Mocha.peach,
    settled = Mocha.green,
    refused = Mocha.red,
)
