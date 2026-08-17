package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog

/**
 * The phone's scanner, desk edition: a place to put the code.
 *
 * Every screen that can scan calls these two, so the desk has to answer —
 * and answer honestly. There is no camera here and no NFC radio, so this
 * does not draw a viewfinder that will never see anything. What a desk
 * *does* have is a clipboard and a keyboard, and a `ducat:` card arrives
 * there constantly: pasted from a chat, an email, a phone across the table
 * reading its own screen aloud. Same door, same `onResult`, so the callers
 * — Contacts, the pay screen, the QR hub — need no desk-specific edit.
 */
@androidx.compose.runtime.Composable
fun QrScanner(
    prompt: String,
    onResult: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    Dialog(onDismissRequest = onDismiss) {
        Surface(
            Modifier.width(520.dp),
            color = MaterialTheme.colorScheme.surface,
            shape = MaterialTheme.shapes.large,
        ) {
            Column(Modifier.padding(20.dp)) {
                QrScannerContent(prompt, onResult, onDismiss)
            }
        }
    }
}

@androidx.compose.runtime.Composable
fun QrScannerContent(
    prompt: String,
    onResult: (String) -> Unit,
    onDismiss: (() -> Unit)? = null,
) {
    val clipboard = LocalClipboardManager.current
    var text by remember { mutableStateOf("") }
    Column(Modifier.fillMaxWidth()) {
        Text(prompt, style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(4.dp))
        Text(
            "This desk has no camera. Paste the code — a ducat: card, or a " +
                "Monero address — and it takes the same path a scan would.",
            style = MaterialTheme.typography.bodySmall,
        )
        Spacer(Modifier.height(12.dp))
        OutlinedTextField(
            value = text,
            onValueChange = { text = it },
            modifier = Modifier.fillMaxWidth(),
            placeholder = { Text("ducat:card/… or an address") },
            maxLines = 3,
        )
        Spacer(Modifier.height(12.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = {
                clipboard.getText()?.text?.let { text = it.trim() }
            }) { Text("Paste") }
            Spacer(Modifier.weight(1f))
            onDismiss?.let {
                TextButton(onClick = it) { Text("Cancel") }
                Spacer(Modifier.width(8.dp))
            }
            Button(
                enabled = text.isNotBlank(),
                onClick = { onResult(text.trim()) },
            ) { Text("Use this") }
        }
    }
}
