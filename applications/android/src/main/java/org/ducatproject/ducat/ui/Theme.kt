package org.ducatproject.ducat.ui

import android.content.Context
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Shapes
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp

/**
 * One radius family, applied everywhere.
 *
 * Mixed corner radii are one of those things nobody names and everybody sees.
 * Inputs and chips sit at the small end, cards at the large end, and dialogs
 * larger still — the same progression PayPal and Venmo use, where the bigger
 * the surface, the softer its corner.
 */
val DucatShapes = Shapes(
    // Rectangles with rounded edges, never pills: a list row two lines tall
    // keeps straight sides at 14dp; at 20dp its ends were half-circles.
    extraSmall = RoundedCornerShape(6.dp),
    small = RoundedCornerShape(8.dp),
    medium = RoundedCornerShape(12.dp),
    large = RoundedCornerShape(14.dp),
    extraLarge = RoundedCornerShape(20.dp),
)

/** What the user chose, if anything. */
enum class ThemeMode { System, Latte, Mocha }

/**
 * Follows the system by default and obeys the user when they say otherwise.
 *
 * Deliberately **not** Material You dynamic colour. A payment app's colours carry
 * meaning — change pending, capacity low, refused — and a scheme derived from
 * whatever wallpaper someone picked cannot promise those stay distinguishable
 * from each other or from the background.
 */
private val LocalDucatColors = staticCompositionLocalOf { LatteMeaning }

val MaterialTheme.ducat: DucatColors
    @Composable get() = LocalDucatColors.current

/**
 * The tones between background and card.
 *
 * These were never set, and that omission was most of why the app looked
 * "almost right": Material 3 components — every Card, the navigation bar, menus
 * — take their fill from `surfaceContainer*`, and an unset role falls back to
 * the baseline Material palette. So cards rendered in Google's grey-violet
 * while everything around them was Catppuccin, one tone off everywhere, on
 * every screen, too subtle to name and impossible not to feel.
 */
private fun latteScheme() = lightColorScheme(
    primary = Latte.mauve,
    onPrimary = Latte.base,
    primaryContainer = Latte.lavender,
    onPrimaryContainer = Latte.crust,
    // Yellow for the send/request action. It is the only control on the bar
    // that does something irreversible, so it gets the one colour nothing else
    // uses — mauve is the app's chrome and would let the button blend into it.
    tertiary = Latte.yellow,
    onTertiary = Latte.crust,
    tertiaryContainer = Latte.yellow,
    onTertiaryContainer = Latte.crust,
    secondary = Latte.sapphire,
    onSecondary = Latte.base,
    background = Latte.base,
    onBackground = Latte.text,
    surface = Latte.mantle,
    onSurface = Latte.text,
    surfaceVariant = Latte.surface0,
    onSurfaceVariant = Latte.subtext0,
    surfaceContainerLowest = Latte.base,
    surfaceContainerLow = Latte.mantle,
    surfaceContainer = Latte.mantle,
    surfaceContainerHigh = Latte.crust,
    surfaceContainerHighest = Latte.surface0,
    surfaceTint = Color.Transparent,
    outline = Latte.overlay0,
    outlineVariant = Latte.surface1,
    error = Latte.red,
    onError = Latte.base,
    // A red-tinted fill rather than another grey. `errorContainer` mapped to a
    // neutral surface, so every warning in the app was grey-on-grey — visually
    // indistinguishable from information, which for a warning is a bug.
    errorContainer = Color(0xFFF6DDE1),
    onErrorContainer = Latte.maroon,
)

private fun mochaScheme() = darkColorScheme(
    primary = Mocha.mauve,
    onPrimary = Mocha.crust,
    primaryContainer = Mocha.surface1,
    onPrimaryContainer = Mocha.lavender,
    tertiary = Mocha.yellow,
    onTertiary = Mocha.crust,
    tertiaryContainer = Mocha.yellow,
    onTertiaryContainer = Mocha.crust,
    secondary = Mocha.sapphire,
    onSecondary = Mocha.crust,
    background = Mocha.base,
    onBackground = Mocha.text,
    surface = Mocha.mantle,
    onSurface = Mocha.text,
    surfaceVariant = Mocha.surface0,
    onSurfaceVariant = Mocha.subtext0,
    surfaceContainerLowest = Mocha.crust,
    surfaceContainerLow = Mocha.mantle,
    surfaceContainer = Mocha.mantle,
    surfaceContainerHigh = Mocha.surface0,
    surfaceContainerHighest = Mocha.surface1,
    surfaceTint = Color.Transparent,
    outline = Mocha.overlay0,
    outlineVariant = Mocha.surface1,
    error = Mocha.red,
    onError = Mocha.crust,
    errorContainer = Color(0xFF45303A),
    onErrorContainer = Mocha.red,
)

@Composable
fun DucatTheme(mode: ThemeMode = ThemeMode.System, content: @Composable () -> Unit) {
    val dark = when (mode) {
        ThemeMode.System -> isSystemInDarkTheme()
        ThemeMode.Latte -> false
        ThemeMode.Mocha -> true
    }
    CompositionLocalProvider(LocalDucatColors provides if (dark) MochaMeaning else LatteMeaning) {
        MaterialTheme(
            colorScheme = if (dark) mochaScheme() else latteScheme(),
            typography = DucatTypography,
            shapes = DucatShapes,
        ) {
            // The bars belong to the theme too. The app draws behind them, so
            // the clock and the battery sit on this background — and unasked,
            // the system painted them white on it.
            SystemBarIcons(dark)
            // The Surface is not decoration. Without it only screens that bring
            // their own Scaffold get a background, and the rest show the
            // Activity's window colour through — which is how onboarding
            // rendered in Latte while the app behind it was Mocha.
            Surface(
                modifier = Modifier.fillMaxSize(),
                color = MaterialTheme.colorScheme.background,
                content = content,
            )
        }
    }
}

/**
 * The choice, persisted.
 *
 * Plain preferences: a theme is not a credential and does not belong in §4.3's
 * encrypted bundle, which exists for things whose loss costs money.
 */
class ThemePreference(context: Context) {
    private val prefs = context.getSharedPreferences("ducat.ui", Context.MODE_PRIVATE)

    var mode: ThemeMode
        get() = runCatching { ThemeMode.valueOf(prefs.getString("theme", null) ?: "System") }
            .getOrDefault(ThemeMode.System)
        set(value) { prefs.edit().putString("theme", value.name).apply() }

    /**
     * Whether setup has been completed.
     *
     * A flag, not the keys: what onboarding *produces* — a persona, a wallet, a
     * backup — belongs in encrypted storage, and putting any of it here would
     * make §4.3's passphrase decorative. This only records that the user has
     * been through the door.
     */
    var onboarded: Boolean
        get() = prefs.getBoolean("onboarded", false)
        set(value) { prefs.edit().putBoolean("onboarded", value).apply() }
}
