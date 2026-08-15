package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
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
                "The transport. A payment travels over an anonymous route, so this " +
                    "has to be up before a tap can go anywhere.",
                style = MaterialTheme.typography.bodySmall,
            )
            Spacer(Modifier.height(12.dp))

            // "Not initialised" and "no peers" look identical in a status line
            // and have nothing to do with each other.
            Line("android bridge", if (androidReady()) "ready" else "NOT SET UP", androidReady())
            Line("node", if (status.running) "running" else "stopped", status.running)
            Line("attached", status.state, status.attached)
            // The distinction that matters, spelled out rather than merged.
            Line(
                "route-capable",
                if (status.publicInternetReady) "yes" else "not yet — no routes until this",
                status.publicInternetReady,
            )
            Line("peers", "${status.peers} live, ${status.reliablePeers} reliable", status.peers > 0u)
            if (status.running && !status.publicInternetReady) {
                Text(
                    "waiting ${elapsed}s — a node has to work out how it is reachable " +
                        "before it can build a route",
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
                    ) { Text(if (starting) "Starting…" else "Start node") }
                } else {
                    OutlinedButton(onClick = {
                        nodeStop()
                        status = NodeStatus(false, false, false, 0u, 0u, "stopped", null)
                        routeResult = null
                    }) { Text("Stop") }
                    Spacer(Modifier.width(8.dp))
                    // The only proof a route can be built is building one.
                    Button(
                        enabled = status.publicInternetReady,
                        onClick = { routeResult = "…" },
                    ) { Text("Test a route") }
                }
            }
        }
    }

    // Startup is off the main thread: it opens a database and touches the
    // network, and a UI that blocks on either is a UI that looks crashed.
    LaunchedEffect(starting) {
        if (!starting) return@LaunchedEffect
        val result = withContext(Dispatchers.IO) {
            runCatching { nodeStart(storageDir, udp = true) }.exceptionOrNull()?.message
        }
        status = withContext(Dispatchers.IO) { nodeStatus() }
        if (result != null) status = status.copy(error = result)
        starting = false
    }

    LaunchedEffect(routeResult) {
        if (routeResult != "…") return@LaunchedEffect
        routeResult = withContext(Dispatchers.IO) {
            runCatching { "route allocated — ${nodeTestRoute()} byte blob" }
                .getOrElse { "route failed: ${it.message}" }
        }
    }
}

@Composable
private fun Line(label: String, value: String, ok: Boolean) {
    Row(Modifier.fillMaxWidth().padding(vertical = 2.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(if (ok) "✓ " else "· ", color = if (ok) MaterialTheme.ducat.settled else MaterialTheme.colorScheme.onSurfaceVariant)
        Text("$label  ", fontWeight = FontWeight.Medium, style = MaterialTheme.typography.bodyMedium)
        Text(value, style = MaterialTheme.typography.bodySmall)
    }
}
