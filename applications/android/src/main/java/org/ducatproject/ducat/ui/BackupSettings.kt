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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import org.ducatproject.ducat.R
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.NameStore
import uniffi.ducat_mobile.BackupInput
import uniffi.ducat_mobile.addressForSpendKey
import uniffi.ducat_mobile.createPersonaSecret
import uniffi.ducat_mobile.exportBackup
import uniffi.ducat_mobile.importBackup
import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.DucatLog

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
    val scope = rememberCoroutineScope()
    var passphrase by remember { mutableStateOf("") }
    var message by remember { mutableStateOf<String?>(null) }
    var restored by remember { mutableStateOf<String?>(null) }
    var pendingImport by remember { mutableStateOf<Uri?>(null) }
    // Export encrypts and writes a file; import decrypts one. Both are heavy
    // enough to jank the frame if run on the main thread, and both used to,
    // with no sign the tap had landed. One flag disables both and shows a
    // spinner so a slow encrypt reads as working, not frozen.
    var busy by remember { mutableStateOf(false) }

    val picker = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument()
    ) { uri -> pendingImport = uri }

    Card(Modifier.fillMaxWidth().padding(vertical = 8.dp), shape = RoundedCornerShape(16.dp)) {
        Column(Modifier.padding(16.dp)) {
            Text(stringResource(R.string.backup_title),
                style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(4.dp))
            Text(
                stringResource(R.string.backup_body),
                style = MaterialTheme.typography.bodySmall,
            )

            Spacer(Modifier.height(12.dp))
            OutlinedTextField(
                value = passphrase,
                onValueChange = { passphrase = it; message = null },
                label = { Text(stringResource(R.string.backup_passphrase)) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )

            Spacer(Modifier.height(12.dp))
            Row {
                Button(
                    enabled = !busy && passphrase.length >= 8 &&
                        spendKeyHex != null && personaSecret != null,
                    onClick = {
                        busy = true; message = null
                        scope.launch {
                            val result = withContext(Dispatchers.IO) {
                                runCatching {
                                    val bytes = exportBackup(
                                        BackupInput(
                                            spendKeyHex!!,
                                            restoreHeight,
                                            // The user's own settings travel with
                                            // their keys: a restore that keeps the
                                            // money and loses the name and the
                                            // privacy choice quietly changed both.
                                            NameStore(context).get(),
                                            ContactStore(context).publishAddress(),
                                            MyProfile(context).toWire(),
                                            ContactStore(context).backupContacts(),
                                            ContactStore(context).backupPrekeys().first,
                                            ContactStore(context).backupPrekeys().second,
                                            ContactStore(context).backupPrekeys().third.toULong(),
                                            ContactStore(context).backupAppState(),
                                        ),
                                        passphrase,
                                        personaSecret!!,
                                    )
                                    val dir = File(context.filesDir, "backups").apply { mkdirs() }
                                    val f = File(dir, "ducat-backup.ducatbak")
                                    f.writeBytes(bytes)
                                    ContactStore(context).markBackupExported()
                                    f to bytes.size
                                }
                            }
                            // The share sheet is an activity; it has to start on
                            // the main thread, so it waits for the IO to finish.
                            result.onSuccess { (f, size) ->
                                share(context, f)
                                message = context.getString(R.string.backup_exported_bytes, size)
                            }.onFailure {
                                message = it.message
                                    ?: context.getString(R.string.backup_export_failed)
                            }
                            busy = false
                        }
                    },
                ) {
                    if (busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                    else Text(stringResource(R.string.backup_export))
                }

                Spacer(Modifier.width(8.dp))
                OutlinedButton(
                    enabled = !busy && passphrase.length >= 8,
                    onClick = { picker.launch(arrayOf("*/*")) },
                ) { Text(stringResource(R.string.backup_import)) }
            }

            message?.let {
                Spacer(Modifier.height(8.dp))
                Text(it, style = MaterialTheme.typography.bodySmall)
            }

            restored?.let {
                Spacer(Modifier.height(12.dp))
                Text(stringResource(R.string.backup_restored_title),
                    fontWeight = FontWeight.SemiBold)
                Text(
                    stringResource(R.string.backup_restored_check),
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
        busy = true
        message = try {
            // Read and decrypt off the main thread — a large bundle would
            // otherwise freeze the frame while it works.
            val r = withContext(Dispatchers.IO) {
                val bytes = context.contentResolver.openInputStream(uri)!!.use { it.readBytes() }
                importBackup(bytes, passphrase)
            }
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
            // The relationships. Threads and tabs from the opaque blob, then
            // the typed contacts as the authoritative overlay — so a bundle
            // from another client still restores everyone.
            ContactStore(context).restoreFromBackup(r)
            // The address is the check that matters. A bundle that decrypts has
            // proved the passphrase, not that it holds the wallet you meant.
            DucatLog.i("Backup", "imported: ${r.contacts.size} contact(s), " +
                "${r.prekeyOneTime.size} prekey(s), escrow ${r.escrowCount}")
            restored = addressForSpendKey(r.spendKeyHex, stagenet = true)
            context.getString(R.string.backup_opened,
                r.escrowCount.toInt(), r.restoreHeight.toLong())
        } catch (t: Throwable) {
            // A wrong passphrase and a tampered file are the same error, on
            // purpose: telling them apart would say whether a guess was close.
            DucatLog.w("Backup", "import failed: ${t.javaClass.simpleName}")
            context.getString(R.string.backup_could_not_open)
        }
        busy = false
    }
}

private fun share(context: Context, file: File) {
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.backups", file)
    val send = Intent(Intent.ACTION_SEND).apply {
        type = "application/octet-stream"
        putExtra(Intent.EXTRA_STREAM, uri)
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    context.startActivity(
        Intent.createChooser(send, context.getString(R.string.backup_share_title)))
}
