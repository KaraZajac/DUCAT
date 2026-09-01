package org.ducatproject.ducat.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.CallEnd
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Button
import androidx.compose.material3.TextButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import org.ducatproject.ducat.Calls
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.R

/**
 * The call, full screen (§16.21): who, what state, and at most two round
 * buttons. A call is the one moment the app should look like a telephone
 * and nothing else.
 */
/** Opens the thread with this contact — the answering-machine button's
 *  road home. Injected by MainActivity; the desk has no thread screen. */
var callOpenThread: (String) -> Unit = {}

@Composable
fun CallScreen() {
    val context = LocalContext.current
    val state = Calls.state
    val contactHex = when (state) {
        is Calls.State.Outgoing -> state.contactHex
        is Calls.State.Incoming -> state.contactHex
        is Calls.State.Active -> state.contactHex
        is Calls.State.NoAnswer -> state.contactHex
        else -> return
    }
    // The answering-machine screen does not linger in a pocket.
    if (state is Calls.State.NoAnswer) {
        LaunchedEffect(state) {
            delay(45_000)
            Calls.dismissNoAnswer()
        }
    }
    val contact = remember(contactHex) {
        ContactStore(context).all().firstOrNull { it.personaHex == contactHex }
    } ?: return

    var nowMs by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(state) {
        while (true) {
            nowMs = System.currentTimeMillis()
            delay(500)
        }
    }

    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surface) {
        Column(
            Modifier.fillMaxSize().padding(32.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.SpaceBetween,
        ) {
            Column(
                Modifier.padding(top = 96.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Box(
                    Modifier.size(96.dp)
                        .background(MaterialTheme.colorScheme.primaryContainer, CircleShape),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        contact.displayName().take(1).uppercase(),
                        style = MaterialTheme.typography.displaySmall,
                        color = MaterialTheme.colorScheme.onPrimaryContainer,
                    )
                }
                Spacer(Modifier.height(20.dp))
                Text(contact.displayName(), style = MaterialTheme.typography.headlineSmall)
                Spacer(Modifier.height(8.dp))
                Text(
                    when (state) {
                        is Calls.State.Outgoing -> stringResource(R.string.call_calling)
                        is Calls.State.Incoming -> stringResource(R.string.call_incoming)
                        is Calls.State.NoAnswer -> stringResource(R.string.call_no_answer)
                        is Calls.State.Active -> {
                            val secs = (nowMs - state.sinceMs) / 1000
                            "%d:%02d".format(secs / 60, secs % 60)
                        }
                        else -> ""
                    },
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                if (state is Calls.State.Active) {
                    Spacer(Modifier.height(4.dp))
                    // Proof of sound in both directions — the honest line a
                    // privacy call owes instead of a signal-bars guess.
                    Text(
                        stringResource(R.string.call_frames, Calls.rxFrames, Calls.txFrames),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }

            // No early return — that shape has crashed Compose here before.
            if (state is Calls.State.NoAnswer) {
                // The answering machine: one primary act (the thread's own
                // recorder is one tap away), one way out.
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Button(onClick = {
                        Calls.dismissNoAnswer()
                        callOpenThread(contactHex)
                    }) {
                        Text(stringResource(R.string.call_leave_message))
                    }
                    Spacer(Modifier.height(16.dp))
                    TextButton(onClick = { Calls.dismissNoAnswer() }) {
                        Text(stringResource(R.string.call_dismiss))
                    }
                }
            } else {
            Row(horizontalArrangement = Arrangement.spacedBy(48.dp)) {
                if (state is Calls.State.Incoming) {
                    FilledIconButton(
                        onClick = {
                            val offer = ContactStore(context).thread(contactHex)
                                .lastOrNull { it.seq == state.offerSeq && !it.outgoing }
                            if (offer != null) Calls.answer(context, contact, offer)
                        },
                        modifier = Modifier.size(72.dp),
                        colors = IconButtonDefaults.filledIconButtonColors(
                            containerColor = MaterialTheme.colorScheme.primary,
                        ),
                    ) {
                        Icon(
                            Icons.Filled.Call,
                            stringResource(R.string.call_answer_btn),
                            Modifier.size(32.dp),
                        )
                    }
                }
                FilledIconButton(
                    onClick = {
                        if (state is Calls.State.Incoming) {
                            val offer = ContactStore(context).thread(contactHex)
                                .lastOrNull { it.seq == state.offerSeq && !it.outgoing }
                            if (offer != null) Calls.decline(context, contact, offer)
                            else Calls.hangUp()
                        } else {
                            Calls.hangUp()
                        }
                    },
                    modifier = Modifier.size(72.dp),
                    colors = IconButtonDefaults.filledIconButtonColors(
                        containerColor = MaterialTheme.colorScheme.error,
                    ),
                ) {
                    Icon(
                        Icons.Filled.CallEnd,
                        stringResource(R.string.call_end_btn),
                        Modifier.size(32.dp),
                    )
                }
            }
            }
        }
    }
}
