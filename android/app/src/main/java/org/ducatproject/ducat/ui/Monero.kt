package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.NodeStore
import uniffi.ducat_mobile.MoneroNodeStatus
import uniffi.ducat_mobile.NodeTrust
import uniffi.ducat_mobile.moneroDefaultNodes
import uniffi.ducat_mobile.moneroPickNode

/**
 * Which Monero node we are using, and what it costs to use it.
 *
 * The panel names the trust level rather than a protocol. Dandelion++ runs in
 * the daemon and has done since 2019, so every live node does it and saying so
 * would distinguish nothing. What differs is whether the node is yours and what
 * carries the request — and that is what a person can actually act on.
 */
@Composable
fun MoneroPanel() {
    val context = LocalContext.current
    val store = remember { NodeStore(context) }
    val scope = rememberCoroutineScope()
    var status by remember { mutableStateOf<MoneroNodeStatus?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var editing by remember { mutableStateOf(false) }
    var ownUrl by remember { mutableStateOf(store.ownUrl() ?: "") }

    fun refresh() {
        busy = true
        error = null
        scope.launch {
            val r = withContext(Dispatchers.IO) {
                runCatching {
                    moneroPickNode(
                        moneroDefaultNodes(store.ownUrl()),
                        "stagenet",
                        8000u,
                    )
                }
            }
            busy = false
            r.onSuccess { status = it; store.rememberLastGood(it.url) }
                .onFailure { status = null; error = it.message ?: "no usable node" }
        }
    }

    LaunchedEffect(Unit) { refresh() }

    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Monero", style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.weight(1f))
                AssistChip(onClick = {}, label = { Text("stagenet") })
                IconButton(onClick = { refresh() }, enabled = !busy) {
                    Icon(Icons.Filled.Refresh, "Check again")
                }
            }
            Spacer(Modifier.height(4.dp))
            Text(
                "Where transactions are broadcast and the chain is read.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(12.dp))

            when {
                busy && status == null -> Row(verticalAlignment = Alignment.CenterVertically) {
                    CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                    Spacer(Modifier.width(10.dp))
                    Text("Finding a node…")
                }
                status != null -> {
                    val s = status!!
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("✓", color = MaterialTheme.ducat.settled)
                        Spacer(Modifier.width(8.dp))
                        Text(s.url, fontFamily = FontFamily.Monospace,
                             style = MaterialTheme.typography.bodySmall)
                    }
                    Spacer(Modifier.height(8.dp))
                    Line("height", "${s.height}")
                    Line("synced", if (s.synced) "yes" else "no — balances would be stale")
                    Line("network", s.nettype)
                    Line("round trip", "${s.rttMs} ms")
                    Spacer(Modifier.height(10.dp))
                    TrustNote(trustOf(store.ownUrl(), s.url))
                }
                else -> Text(
                    error ?: "no node",
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
            }

            Spacer(Modifier.height(14.dp))
            if (!editing) {
                TextButton(onClick = { editing = true }) {
                    Text(if (store.ownUrl() != null) "Change your node" else "Use your own node")
                }
            } else {
                OutlinedTextField(
                    value = ownUrl,
                    onValueChange = { ownUrl = it },
                    label = { Text("Your node") },
                    placeholder = { Text("http://192.168.1.10:38081") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(8.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = {
                        store.setOwnUrl(ownUrl.ifBlank { null })
                        editing = false
                        refresh()
                    }) { Text("Use it") }
                    if (store.ownUrl() != null) {
                        OutlinedButton(onClick = {
                            store.setOwnUrl(null); ownUrl = ""; editing = false; refresh()
                        }) { Text("Back to public") }
                    }
                    TextButton(onClick = { editing = false }) { Text("Cancel") }
                }
            }
        }
    }
}

private fun trustOf(own: String?, inUse: String): NodeTrust = when {
    own != null && own.trim() == inUse -> NodeTrust.OWN
    inUse.contains(".onion") -> NodeTrust.ONION
    else -> NodeTrust.PUBLIC_CLEARNET
}

@Composable
private fun TrustNote(trust: NodeTrust) {
    val (title, body, colour) = when (trust) {
        NodeTrust.OWN -> Triple(
            "Your node",
            "It learns nothing about you that you did not already know.",
            MaterialTheme.ducat.settled,
        )
        NodeTrust.ONION -> Triple(
            "Someone else's node, over Tor",
            "It sees your transactions but not where they came from.",
            MaterialTheme.colorScheme.onSurfaceVariant,
        )
        NodeTrust.PUBLIC_CLEARNET -> Triple(
            "Someone else's node",
            "It sees your address and your transactions together. Dandelion++ " +
                "hides a transaction's origin from the rest of the network — it " +
                "cannot hide it from the node you handed it to. Running your own " +
                "is the fix.",
            MaterialTheme.ducat.lowCapacity,
        )
    }
    Column {
        Text(title, style = MaterialTheme.typography.labelLarge, color = colour)
        Spacer(Modifier.height(2.dp))
        Text(
            body,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun Line(k: String, v: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 2.dp)) {
        Text(k, style = MaterialTheme.typography.bodySmall,
             color = MaterialTheme.colorScheme.onSurfaceVariant)
        Spacer(Modifier.weight(1f))
        Text(v, style = MaterialTheme.typography.bodySmall)
    }
}
