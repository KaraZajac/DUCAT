package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import org.ducatproject.ducat.R
import org.ducatproject.ducat.saidWhy
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import uniffi.ducat_mobile.NodeStatus
import uniffi.ducat_mobile.androidReady
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus
import uniffi.ducat_mobile.nodeStop
import uniffi.ducat_mobile.nodeTestRoute

/**
 * The network panel — built to be troubleshot from, not to look reassuring.
 *
 * A single "connected" light would be actively misleading here. **Attachment is
 * not readiness**: a node can be attached, talking to peers, and still unable to
 * allocate a private route because it has not determined its network class — and
 * every DUCAT reach mode needs a route. So the two are shown separately, and the
 * route test is a button rather than an inference, because the only proof a
 * route can be built is building one.
 */
@Composable
fun NetworkPanel(storageDir: String) {
    val context = androidx.compose.ui.platform.LocalContext.current
    var status by remember { mutableStateOf(NodeStatus(false, false, false, 0u, 0u, "stopped", null)) }
    var starting by remember { mutableStateOf(false) }
    var routeResult by remember { mutableStateOf<String?>(null) }
    var elapsed by remember { mutableStateOf(0) }

    // The node is started by the Application, so this screen begins by asking
    // what is already happening rather than by offering to start something.
    LaunchedEffect(Unit) {
        status = withContext(Dispatchers.IO) { nodeStatus() }
    }

    // Poll while running. Readiness takes seconds to minutes, and a screen that
    // shows one sample tells you nothing about whether it is progressing.
    LaunchedEffect(status.running) {
        while (status.running) {
            delay(2000)
            status = withContext(Dispatchers.IO) { nodeStatus() }
            elapsed += 2
        }
    }

    Card(Modifier.fillMaxWidth().padding(vertical = 8.dp), shape = RoundedCornerShape(16.dp)) {
        Column(Modifier.padding(16.dp)) {
            Text("Veilid", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(4.dp))
            Text(
                stringResource(R.string.net_veilid_body),
                style = MaterialTheme.typography.bodySmall,
            )
            Spacer(Modifier.height(12.dp))

            // "Not initialised" and "no peers" look identical in a status line
            // and have nothing to do with each other.
            Line(
                stringResource(R.string.net_line_bridge),
                stringResource(
                    if (androidReady()) R.string.net_ready else R.string.net_not_set_up),
                androidReady(),
            )
            Line(
                stringResource(R.string.net_line_node),
                stringResource(
                    if (status.running) R.string.net_running else R.string.net_stopped),
                status.running,
            )
            Line(stringResource(R.string.net_line_attached), status.state, status.attached)
            // The distinction that matters, spelled out rather than merged.
            Line(
                stringResource(R.string.net_line_route_capable),
                if (status.publicInternetReady) stringResource(R.string.net_yes)
                else stringResource(R.string.net_not_yet_routes),
                status.publicInternetReady,
            )
            Line(
                stringResource(R.string.net_line_peers),
                stringResource(R.string.net_peers_value,
                    status.peers.toString(), status.reliablePeers.toString()),
                status.peers > 0u,
            )
            if (status.running && !status.publicInternetReady) {
                Text(
                    stringResource(R.string.net_waiting, elapsed),
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            status.error?.let {
                Spacer(Modifier.height(8.dp))
                Text(it, color = MaterialTheme.colorScheme.error, fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.bodySmall)
            }

            routeResult?.let {
                Spacer(Modifier.height(8.dp))
                Text(it, style = MaterialTheme.typography.bodySmall, fontFamily = FontFamily.Monospace)
            }

            Spacer(Modifier.height(12.dp))
            Row {
                if (!status.running) {
                    Button(
                        enabled = !starting,
                        onClick = {
                            starting = true
                            routeResult = null
                            elapsed = 0
                        },
                    ) {
                        Text(stringResource(
                            if (starting) R.string.net_starting else R.string.net_start_node))
                    }
                } else {
                    OutlinedButton(onClick = {
                        nodeStop()
                        status = NodeStatus(false, false, false, 0u, 0u, "stopped", null)
                        routeResult = null
                    }) { Text(stringResource(R.string.net_stop)) }
                    Spacer(Modifier.width(8.dp))
                    // The only proof a route can be built is building one.
                    Button(
                        // Disabled while a probe is in flight (routeResult "…")
                        // so a second tap cannot restart it mid-test.
                        enabled = status.publicInternetReady && routeResult != "…",
                        onClick = { routeResult = "…" },
                    ) {
                        Text(stringResource(
                            if (routeResult == "…") R.string.net_testing
                            else R.string.net_test_route))
                    }
                }
            }
        }
    }

    // Startup is off the main thread: it opens a database and touches the
    // network, and a UI that blocks on either is a UI that looks crashed.
    LaunchedEffect(starting) {
        if (!starting) return@LaunchedEffect
        val result = withContext(Dispatchers.IO) {
            runCatching { nodeStart(storageDir, udp = true) }.exceptionOrNull()?.saidWhy()
        }
        status = withContext(Dispatchers.IO) { nodeStatus() }
        if (result != null) status = status.copy(error = startupNote(context, result))
        starting = false
    }

    LaunchedEffect(routeResult) {
        if (routeResult != "…") return@LaunchedEffect
        routeResult = withContext(Dispatchers.IO) {
            runCatching {
                context.getString(R.string.net_route_ok, nodeTestRoute().toString())
            }.getOrElse { context.getString(R.string.net_route_failed, it.saidWhy() ?: "?") }
        }
    }
}

/**
 * A failed start, with a sentence in front of the machinery.
 *
 * This panel is the only place a person can find out why nothing works, so
 * what it said mattered more than its length: `v1=startup: Internal: Could
 * not initialize the protected store.` is accurate, is the whole story, and
 * tells somebody holding a phone nothing they can act on — least of all that
 * their money is not the thing that broke.
 *
 * The raw line stays underneath. It is what gets pasted into a bug report,
 * and the wording above it is a guess about which failure this is; the guess
 * must never replace the evidence.
 */
private fun startupNote(context: android.content.Context, raw: String): String {
    val msg = bridgeMessage(raw).removePrefix("startup:").trim()
    if (!msg.contains("protected store", ignoreCase = true)) return msg
    return context.getString(R.string.net_keystore_failed) + "\n\n" + msg
}

@Composable
private fun Line(label: String, value: String, ok: Boolean) {
    Row(Modifier.fillMaxWidth().padding(vertical = 2.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(if (ok) "✓" else "·", color = if (ok) MaterialTheme.ducat.settled else MaterialTheme.colorScheme.onSurfaceVariant)
        Spacer(Modifier.width(6.dp))
        Text(label, fontWeight = FontWeight.Medium, style = MaterialTheme.typography.bodyMedium)
        Spacer(Modifier.width(10.dp))
        Text(value, style = MaterialTheme.typography.bodySmall)
    }
}
