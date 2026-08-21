package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.DeviceLock
import org.ducatproject.ducat.Pin
import org.ducatproject.ducat.R

/**
 * Ask for the PIN, and do not let go until it is right.
 *
 * Also *sets* one when there is none, which is what makes this safe to put in
 * front of every payment: a device that onboarded before PINs existed meets
 * the gate at its first spend and is asked to choose one rather than being
 * quietly waved through. There is no third path where money leaves without
 * somebody proving they are the owner.
 *
 * The check runs off the main thread on purpose — deriving the verifier is
 * two hundred thousand rounds, which is the point of it, and a UI thread that
 * did it would freeze for as long as it took.
 */
@Composable
fun PinGate(
    open: Boolean,
    onDismiss: () -> Unit,
    onPassed: () -> Unit,
    /**
     * The one line under the title, saying what is behind the gate.
     *
     * Defaults to the spend warning because that is what most of these guard,
     * but not all of them do, and the default was being told to people it was
     * not true of: opening the kiosk's staff panel and switching operating
     * modes both announced "Money is about to leave this phone." Nothing was
     * leaving. A gate that cries wolf about a spend is a gate people learn to
     * tap through.
     */
    why: Int = R.string.pin_ask_body,
) {
    if (!open) return
    val context = LocalContext.current
    val setting = remember { !Pin.isSet(context) }
    // The phone's own lock, where there is one. Never offered while *setting*
    // a PIN — the point of that step is that this app has a secret of its own,
    // and a fingerprint cannot stand in for choosing one.
    val deviceLock = remember { !setting && DeviceLock.available(context) }
    var asking by remember { mutableStateOf(false) }
    // Raise it without being asked once they have used it once. The PIN stays
    // on screen underneath, so declining the prompt is not a dead end.
    LaunchedEffect(deviceLock) {
        if (deviceLock && DeviceLock.preferred(context)) asking = true
    }
    var entered by remember { mutableStateOf("") }
    var again by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    // Ticks while a lockout runs, so the wait counts down in front of the
    // person waiting rather than sitting on a stale number.
    var lockedFor by remember { mutableLongStateOf(Pin.lockedFor(context)) }
    LaunchedEffect(open) {
        while (true) {
            lockedFor = Pin.lockedFor(context)
            delay(1_000)
        }
    }

    // The system prompt is modal and owns the screen while it is up; this
    // just starts it and waits for the one answer that opens the gate.
    LaunchedEffect(asking) {
        if (!asking) return@LaunchedEffect
        DeviceLock.backend?.prompt(
            context,
            context.getString(R.string.pin_device_title),
            context.getString(R.string.pin_device_subtitle),
        ) { ok ->
            asking = false
            if (ok) {
                DeviceLock.remember(context, true)
                onPassed()
            }
        } ?: run { asking = false }
    }

    val digitsOk = entered.length in Pin.MIN_DIGITS..Pin.MAX_DIGITS
    val ready = if (setting) digitsOk && again == entered else digitsOk && lockedFor == 0L

    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = {
            Text(
                stringResource(if (setting) R.string.pin_set_title else R.string.pin_ask_title),
            )
        },
        text = {
            Column {
                Text(
                    stringResource(
                        if (setting) R.string.pin_set_body else why,
                    ),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(12.dp))
                OutlinedTextField(
                    value = entered,
                    onValueChange = {
                        problem = null
                        entered = Amounts.typedNumber(it)
                            .filter { c -> c in '0'..'9' }.take(Pin.MAX_DIGITS)
                    },
                    label = { Text(stringResource(R.string.pin_label)) },
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                        keyboardType = KeyboardType.NumberPassword,
                    ),
                    modifier = Modifier.fillMaxWidth(),
                )
                if (setting) {
                    Spacer(Modifier.height(8.dp))
                    OutlinedTextField(
                        value = again,
                        onValueChange = {
                            problem = null
                            again = Amounts.typedNumber(it)
                            .filter { c -> c in '0'..'9' }.take(Pin.MAX_DIGITS)
                        },
                        label = { Text(stringResource(R.string.pin_again)) },
                        singleLine = true,
                        visualTransformation = PasswordVisualTransformation(),
                        keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                            keyboardType = KeyboardType.NumberPassword,
                        ),
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Spacer(Modifier.height(8.dp))
                    // Said while they can still act on it, not in a help page
                    // after they have forgotten: this PIN cannot be reset from
                    // inside the app, because anything that could reset it
                    // would be the way past it.
                    Text(
                        stringResource(R.string.pin_no_reset),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                if (deviceLock && !asking) {
                    Spacer(Modifier.height(12.dp))
                    // A button rather than a setting. Somebody who wants this
                    // presses it once and is never asked again; somebody who
                    // does not never goes looking through a settings screen to
                    // turn it off.
                    TextButton(
                        enabled = !busy && lockedFor == 0L,
                        onClick = { problem = null; asking = true },
                        modifier = Modifier.fillMaxWidth(),
                    ) { Text(stringResource(R.string.pin_use_device_lock)) }
                }
                if (lockedFor > 0) {
                    Spacer(Modifier.height(8.dp))
                    Text(
                        stringResource(
                            R.string.pin_locked,
                            humanDuration(context, lockedFor),
                        ),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error,
                    )
                }
                problem?.let {
                    Spacer(Modifier.height(8.dp))
                    Text(
                        it,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error,
                    )
                }
            }
        },
        confirmButton = {
            TextButton(
                enabled = ready && !busy,
                // Setting `busy` is the whole action: the effect below picks
                // it up and does the slow part off this thread.
                onClick = { problem = null; busy = true },
            ) { Text(stringResource(if (setting) R.string.pin_save else R.string.pin_confirm)) }
        },
        dismissButton = {
            TextButton(enabled = !busy, onClick = onDismiss) {
                Text(stringResource(R.string.pin_cancel))
            }
        },
    )

    // The work itself, off the main thread, driven by `busy` so the button
    // stays a button and the dialog stays responsive while PBKDF2 runs.
    LaunchedEffect(busy) {
        if (!busy) return@LaunchedEffect
        if (setting) {
            withContext(Dispatchers.Default) { Pin.set(context, entered) }
            busy = false
            onPassed()
            return@LaunchedEffect
        }
        val verdict = withContext(Dispatchers.Default) { Pin.verify(context, entered) }
        busy = false
        when (verdict) {
            is Pin.Verdict.Ok -> onPassed()
            is Pin.Verdict.Wrong -> {
                entered = ""
                problem = context.resources.getQuantityString(
                    R.plurals.pin_wrong, verdict.triesLeft, verdict.triesLeft,
                )
            }
            is Pin.Verdict.Locked -> {
                entered = ""
                lockedFor = verdict.secondsLeft
            }
            // No PIN, but `setting` was false — the store changed underneath
            // us. Ask them to set one rather than letting the payment past.
            is Pin.Verdict.Unset -> problem = context.getString(R.string.pin_gone)
        }
    }
}
