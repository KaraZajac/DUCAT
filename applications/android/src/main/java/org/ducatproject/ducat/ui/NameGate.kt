package org.ducatproject.ducat.ui

import android.content.Context
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import org.ducatproject.ducat.NameStore
import org.ducatproject.ducat.R

/**
 * Ask what to call you, at the moment somebody is about to find out.
 *
 * The display name is optional, deliberately: onboarding says so, the profile
 * screen says so, and a person who wants to trade without one is entitled to.
 * What was not deliberate is that leaving it blank is *silent*. It travels on
 * every handshake (§16.9), so a phone with no name asserts none, and the far
 * side stores none — which is how somebody who had just agreed to repair a
 * bicycle for a stranger found their whole screen, from the chat title to the
 * escrow they were funding, calling that stranger "Unnamed contact". Neither
 * end was ever told that was going to happen.
 *
 * Onboarding is the wrong place to fix it. It already asks, in a step whose
 * own documentation calls it the worst possible time to be reading about
 * linkability, and it is asked of somebody who has met nobody yet and has no
 * idea what the answer is for. This asks at the first introduction instead,
 * when the other person is on the screen behind the dialog and the question
 * answers itself.
 *
 * Modelled on [PinGate], which had the same shape of problem — a device that
 * onboarded before PINs existed meeting the gate at its first spend rather
 * than being quietly waved through — and the same answer.
 *
 * Asked once. Staying anonymous is a real answer, and nagging is not how the
 * rest of this app treats one; the drawer goes on offering "Set a profile
 * name" for anyone who changes their mind.
 */
@Composable
fun NameGate(
    open: Boolean,
    onDismiss: () -> Unit,
    onNamed: () -> Unit,
) {
    if (!open) return
    val context = LocalContext.current
    var name by remember { mutableStateOf("") }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.name_gate_title)) },
        text = {
            Column {
                Text(
                    stringResource(R.string.name_gate_body),
                    style = MaterialTheme.typography.bodyMedium,
                )
                Spacer(Modifier.height(12.dp))
                OutlinedTextField(
                    value = name,
                    onValueChange = { if (it.length <= 32) name = it },
                    label = { Text(stringResource(R.string.name_gate_label)) },
                    supportingText = { CharCounter(name.length, 32) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(8.dp))
                Text(
                    stringResource(R.string.name_gate_later),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.outline,
                )
            }
        },
        confirmButton = {
            TextButton(
                enabled = name.isNotBlank(),
                onClick = {
                    val store = NameStore(context)
                    store.put(name.trim())
                    store.markAsked()
                    onNamed()
                },
            ) { Text(stringResource(R.string.name_gate_use)) }
        },
        dismissButton = {
            // Quieter than the other one, and it still goes through: the
            // introduction happens either way. What it must not do is happen
            // *silently*, which is the whole of the original bug.
            TextButton(
                onClick = {
                    NameStore(context).markAsked()
                    onNamed()
                },
            ) { Text(stringResource(R.string.name_gate_skip)) }
        },
    )
}

/**
 * Should the gate open before this introduction?
 *
 * Only when there is nothing to introduce ourselves with *and* nobody has
 * been asked yet — see [NameStore.needed] and [NameStore.asked].
 */
fun nameGateNeeded(context: Context): Boolean =
    NameStore(context).let { it.needed() && !it.asked() }
