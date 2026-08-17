// Android's back gesture is a system behaviour with no desktop equivalent:
// a window has a close button and an Escape key, and screens here are hosted
// panels rather than a back stack. The handler is registered so the phone's
// sources compile and read the same, and the desk drives the same lambdas
// from its own affordances.

package androidx.activity.compose

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect

@Composable
fun BackHandler(enabled: Boolean = true, onBack: () -> Unit) {
    DisposableEffect(enabled, onBack) {
        if (enabled) Back.stack.add(onBack)
        onDispose { Back.stack.remove(onBack) }
    }
}

object Back {
    val stack = mutableListOf<() -> Unit>()

    /** Escape, or a desk Back button: the innermost handler wins, as on a phone. */
    fun pop(): Boolean {
        val h = stack.lastOrNull() ?: return false
        h()
        return true
    }
}
