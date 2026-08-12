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

private fun latteScheme() = lightColorScheme(
    primary = Latte.mauve,
    onPrimary = Latte.base,
    primaryContainer = Latte.lavender,
    onPrimaryContainer = Latte.crust,
    secondary = Latte.sapphire,
    onSecondary = Latte.base,
    background = Latte.base,
    onBackground = Latte.text,
    surface = Latte.mantle,
    onSurface = Latte.text,
    surfaceVariant = Latte.surface0,
    onSurfaceVariant = Latte.subtext0,
    outline = Latte.overlay0,
    error = Latte.red,
    onError = Latte.base,
    errorContainer = Latte.crust,
    onErrorContainer = Latte.maroon,
)

private fun mochaScheme() = darkColorScheme(
    primary = Mocha.mauve,
    onPrimary = Mocha.crust,
    primaryContainer = Mocha.surface1,
    onPrimaryContainer = Mocha.lavender,
    secondary = Mocha.sapphire,
    onSecondary = Mocha.crust,
    background = Mocha.base,
    onBackground = Mocha.text,
    surface = Mocha.mantle,
    onSurface = Mocha.text,
    surfaceVariant = Mocha.surface0,
    onSurfaceVariant = Mocha.subtext0,
    outline = Mocha.overlay0,
    error = Mocha.red,
    onError = Mocha.crust,
    errorContainer = Mocha.surface0,
    onErrorContainer = Mocha.maroon,
)

@Composable
fun DucatTheme(mode: ThemeMode = ThemeMode.System, content: @Composable () -> Unit) {
    val dark = when (mode) {
        ThemeMode.System -> isSystemInDarkTheme()
        ThemeMode.Latte -> false
        ThemeMode.Mocha -> true
    }
    CompositionLocalProvider(LocalDucatColors provides if (dark) MochaMeaning else LatteMeaning) {
        MaterialTheme(colorScheme = if (dark) mochaScheme() else latteScheme()) {
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
