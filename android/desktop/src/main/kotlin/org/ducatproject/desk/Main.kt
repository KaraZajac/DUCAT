package org.ducatproject.desk

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Window
import androidx.compose.ui.window.application
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.File
import uniffi.ducat_mobile.NodeStatus
import uniffi.ducat_mobile.createPersonaSecret
import uniffi.ducat_mobile.createWallet
import uniffi.ducat_mobile.moneroDefaultNodes
import uniffi.ducat_mobile.moneroProbe
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus
import uniffi.ducat_mobile.personaPublicHex

/**
 * DUCAT Desk — the desktop client, v1: prove the whole stack stands up on a
 * plain JVM. One window, three truths: the Veilid node's attachment, the
 * wallet's receiving address, the persona contacts key by. Everything else
 * (cards, chat, ceremonies) arrives by porting the Android stores off
 * `Context`; this file deliberately holds only what has no phone in it.
 */

/** `$XDG_DATA_HOME/ducat-desk`, or the home-dir fallback. */
private fun dataDir(): File {
    val base = System.getenv("XDG_DATA_HOME")?.takeIf { it.isNotEmpty() }
        ?: "${System.getProperty("user.home")}/.local/share"
    return File(base, "ducat-desk").apply { mkdirs() }
}

/**
 * The desk's identity, one JSON file. The Android app keeps this in
 * SharedPreferences; a desk keeps files, and §4.3's backup rules apply to
 * this one the moment real money lands on it.
 */
private class DeskState(private val file: File) {
    private val o: JSONObject =
        if (file.exists()) JSONObject(file.readText()) else JSONObject()

    private fun save() = file.writeText(o.toString(2))

    fun walletAddress(): String? = o.optString("wallet_address").takeIf { it.isNotEmpty() }

    fun ensureWallet(): Pair<String, String>? {
        walletAddress()?.let { return it to "loaded" }
        // A wallet born without a real restore height rescans from genesis —
        // 106 measured hours — so creation waits for a node that answers.
        val node = moneroDefaultNodes(null).firstOrNull { c ->
            runCatching { moneroProbe(c.url, 4000u).let { it.reachable && it.height > 0uL } }
                .getOrDefault(false)
        } ?: return null
        val tip = moneroProbe(node.url, 4000u).height
        val w = createWallet(tip, stagenet = true)
        o.put("wallet_address", w.address)
        o.put("wallet_spend", w.spendKeyHex)
        o.put("wallet_height", w.restoreHeight.toLong())
        o.put("wallet_stagenet", true)
        save()
        return w.address to "created at height ${w.restoreHeight}"
    }

    fun personaHex(): String {
        o.optString("persona_secret").takeIf { it.isNotEmpty() }?.let {
            return personaPublicHex(java.util.Base64.getDecoder().decode(it))
        }
        val fresh = createPersonaSecret()
        o.put("persona_secret", java.util.Base64.getEncoder().encodeToString(fresh))
        save()
        return personaPublicHex(fresh)
    }
}

fun main() = application {
    Window(onCloseRequest = ::exitApplication, title = "DUCAT Desk") {
        var status by remember { mutableStateOf<NodeStatus?>(null) }
        var wallet by remember { mutableStateOf<Pair<String, String>?>(null) }
        var persona by remember { mutableStateOf<String?>(null) }
        var error by remember { mutableStateOf<String?>(null) }

        LaunchedEffect(Unit) {
            withContext(Dispatchers.IO) {
                runCatching {
                    val dir = dataDir()
                    nodeStart(File(dir, "veilid").absolutePath)
                    val state = DeskState(File(dir, "desk.json"))
                    persona = state.personaHex()
                    wallet = state.ensureWallet()
                }.onFailure { error = it.message }
            }
            while (true) {
                status = runCatching { nodeStatus() }.getOrNull()
                delay(2_000)
            }
        }

        MaterialTheme(colorScheme = darkColorScheme()) {
            Surface(Modifier.fillMaxSize()) {
                Column(Modifier.padding(24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text("DUCAT Desk", style = MaterialTheme.typography.headlineMedium)

                    val s = status
                    Text(
                        when {
                            error != null -> "node: $error"
                            s == null -> "node: starting…"
                            s.publicInternetReady ->
                                "node: ready — ${s.reliablePeers}/${s.peers} peers"
                            s.attached -> "node: attaching… (${s.peers} peers)"
                            else -> "node: ${s.state}"
                        },
                        style = MaterialTheme.typography.bodyLarge,
                    )

                    persona?.let {
                        Text("persona", style = MaterialTheme.typography.labelMedium)
                        SelectionContainer { Text(it, style = MaterialTheme.typography.bodySmall) }
                    }

                    wallet?.let { (addr, how) ->
                        Text("wallet ($how)", style = MaterialTheme.typography.labelMedium)
                        SelectionContainer { Text(addr, style = MaterialTheme.typography.bodySmall) }
                    } ?: Text(
                        "wallet: waiting for a Monero node to answer…",
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
        }
    }
}
