// `rememberLauncherForActivityResult` — the Compose half of picking a file.
// A phone launches an activity and waits for a result; here the contract
// runs the dialog inline and hands the same value to the same callback.

package androidx.activity.compose

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.activity.result.contract.ActivityResultContract

class ManagedActivityResultLauncher<I, O> internal constructor(
    private val contract: ActivityResultContract<I, O>,
    private val onResult: (O) -> Unit,
) {
    fun launch(input: I) {
        // Off the composition's thread: a modal file dialog would otherwise
        // block the frame it was opened from.
        Thread {
            val out = runCatching { contract.run(input) }.getOrNull()
            if (out != null) onResult(out)
        }.start()
    }
}

@Composable
fun <I, O> rememberLauncherForActivityResult(
    contract: ActivityResultContract<I, O>,
    onResult: (O) -> Unit,
): ManagedActivityResultLauncher<I, O> = remember(contract) {
    ManagedActivityResultLauncher(contract, onResult)
}
