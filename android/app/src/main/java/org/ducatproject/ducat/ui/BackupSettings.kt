package org.ducatproject.ducat.ui

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import java.io.File
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.NameStore
import uniffi.ducat_mobile.BackupInput
import uniffi.ducat_mobile.addressForSpendKey
import uniffi.ducat_mobile.createPersonaSecret
import uniffi.ducat_mobile.exportBackup
import uniffi.ducat_mobile.importBackup
import org.ducatproject.ducat.MyProfile

/**
 * Backup, after onboarding.
 *
 * Export exists here as well as in setup because §4.3.3 makes the bundle's
 * escrow shares the one part with a **freshness** requirement: everything else
 * stays valid indefinitely, but a bundle exported before an escrow opened does
 * not contain it. A backup you can only make once is a backup that goes stale.
 *
 * Import exists because a backup nobody has ever restored is a backup nobody
 * knows works. The screen therefore shows the **address** the restored key
 * controls, so a user can check it against one they recognise before trusting it.
 */
@Composable
fun BackupSettings(spendKeyHex: String?, restoreHeight: ULong, personaSecret: ByteArray?) {
    val context = LocalContext.current
    var passphrase by remember { mutableStateOf("") }
    var message by remember { mutableStateOf<String?>(null) }
    var restored by remember { mutableStateOf<String?>(null) }
    var pendingImport by remember { mutableStateOf<Uri?>(null) }

    val picker = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument()
    ) { uri -> pendingImport = uri }

    Card(Modifier.fillMaxWidth().padding(vertical = 8.dp), shape = RoundedCornerShape(16.dp)) {
        Column(Modifier.padding(16.dp)) {
            Text("Backup", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(4.dp))
            Text(
                "One encrypted file holding your identity, your keys and your " +
                    "settings. Export a fresh one whenever something important " +
                    "changes.",
                style = MaterialTheme.typography.bodySmall,
            )

            Spacer(Modifier.height(12.dp))
            OutlinedTextField(
                value = passphrase,
                onValueChange = { passphrase = it; message = null },
                label = { Text("Passphrase") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )

            Spacer(Modifier.height(12.dp))
            Row {
                Button(
                    enabled = passphrase.length >= 8 && spendKeyHex != null && personaSecret != null,
                    onClick = {
                        message = try {
                            val bytes = exportBackup(
                                BackupInput(
                                    spendKeyHex!!,
                                    restoreHeight,
                                    // The user's own settings travel with their
                                    // keys: a restore that keeps the money and
                                    // loses the name and the privacy choice is
                                    // a restore that quietly changed both.
                                    NameStore(context).get(),
                                    ContactStore(context).publishAddress(),
                                    MyProfile(context).toWire(),
                                ),
                                passphrase,
                                personaSecret!!,
                            )
                            val dir = File(context.filesDir, "backups").apply { mkdirs() }
                            val f = File(dir, "ducat-backup.ducatbak")
                            f.writeBytes(bytes)
                            share(context, f)
                            "Exported ${bytes.size} bytes"
                        } catch (t: Throwable) {
                            t.message ?: "export failed"
                        }
                    },
                ) { Text("Export") }

                Spacer(Modifier.width(8.dp))
                OutlinedButton(
                    enabled = passphrase.length >= 8,
                    onClick = { picker.launch(arrayOf("*/*")) },
                ) { Text("Import") }
            }

            message?.let {
                Spacer(Modifier.height(8.dp))
                Text(it, style = MaterialTheme.typography.bodySmall)
            }

            restored?.let {
                Spacer(Modifier.height(12.dp))
                Text("Restored wallet", fontWeight = FontWeight.SemiBold)
                Text(
                    "Check this address is one you recognise before trusting it.",
                    style = MaterialTheme.typography.bodySmall,
                )
                Text(it, fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.bodySmall)
            }
        }
    }

    LaunchedEffect(pendingImport) {
        val uri = pendingImport ?: return@LaunchedEffect
        pendingImport = null
        message = try {
            val bytes = context.contentResolver.openInputStream(uri)!!.use { it.readBytes() }
            val r = importBackup(bytes, passphrase)
            // Settings come back too. A restore that keeps the money and drops
            // the name and the privacy choice has quietly changed both, and the
            // user has no way to notice — publishing especially, where the
            // wrong direction is a silent disclosure.
            r.displayName?.let { NameStore(context).put(it) }
            ContactStore(context).setPublishAddress(r.publishPayto)
            // §16.9's profile with it. A persona that comes back with the right
            // money and no face is not the same person to anyone who knew them,
            // and nothing else in the app would report that it had been lost.
            MyProfile(context).let { p ->
                p.setAvatar(r.profile.avatar)
                p.setEmail(r.profile.email)
                p.setPhone(r.profile.phone)
                p.setSignal(r.profile.signal)
                p.setPronouns(r.profile.pronouns?.toInt())
            }
            // The address is the check that matters. A bundle that decrypts has
            // proved the passphrase, not that it holds the wallet you meant.
            restored = addressForSpendKey(r.spendKeyHex, stagenet = true)
            "Opened — ${r.escrowCount} escrow share(s), restore height ${r.restoreHeight}"
        } catch (t: Throwable) {
            // A wrong passphrase and a tampered file are the same error, on
            // purpose: telling them apart would say whether a guess was close.
            "Could not open it — wrong passphrase, or the file has been altered"
        }
    }
}

private fun share(context: Context, file: File) {
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.backups", file)
    val send = Intent(Intent.ACTION_SEND).apply {
        type = "application/octet-stream"
        putExtra(Intent.EXTRA_STREAM, uri)
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    context.startActivity(Intent.createChooser(send, "Save your DUCAT backup"))
}
