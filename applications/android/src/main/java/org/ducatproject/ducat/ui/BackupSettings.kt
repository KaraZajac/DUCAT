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
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Ceremony
import org.ducatproject.ducat.NameStore
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.WalletStore
import uniffi.ducat_mobile.BackupInput
import uniffi.ducat_mobile.addressForSpendKey
import uniffi.ducat_mobile.createPersonaSecret
import uniffi.ducat_mobile.exportBackup
import uniffi.ducat_mobile.importBackup
import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.saidWhy

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
                // The rule both buttons are enforcing, said out loud — the
                // same way onboarding says it, with the same string.
                //
                // Export and Import stay dead below eight characters and
                // nothing on this screen mentioned eight, so somebody typing a
                // short passphrase got two grey buttons and no reason. Green
                // when it is satisfied, so the field answers the question
                // rather than just repeating the demand.
                supportingText = {
                    // Graded, not merely measured. Clearing the eight-byte
                    // floor used to turn this green and say "Good" — an
                    // endorsement of the weakest passphrase the format will
                    // accept, for a file that holds the spend key, the persona
                    // and every relationship, and whose whole purpose is to be
                    // kept somewhere else where an attacker can grind at it.
                    PassphraseNote(passphrase)
                },
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
                                            // §4.3.3, and the reason this screen
                                            // talks about freshness. They live in
                                            // their own store, which is how they
                                            // came to be left out of the one this
                                            // is assembled from.
                                            Ceremony.backupShares(context),
                                            // The compartments, primary first —
                                            // a restore is becoming this phone,
                                            // every hat included.
                                            PersonaStore(context).backupPersonas(),
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
                                message = it.saidWhy()
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
        busy = true
        // NonCancellable, and `pendingImport` is cleared at the very end.
        //
        // This effect is keyed on `pendingImport`, so clearing it *first* — as
        // this did — changed the key and cancelled the coroutine that was
        // running the restore. Argon2id is deliberately slow, so it lost that
        // race every time: every import died as LeftCompositionCancellationException
        // and was reported to the user as "wrong passphrase, or the file has
        // been altered", which is a lie about their backup at the worst
        // possible moment.
        //
        // The writes below are the other reason. A restore puts back the name,
        // the publish choice, the profile and every contact; interrupted
        // halfway it leaves a device that is neither the old identity nor the
        // new one, with nothing to say so.
        val outcome = withContext(Dispatchers.IO + NonCancellable) {
            runCatching {
                val bytes = context.contentResolver.openInputStream(uri)!!
                    .use { it.readBytes() }
                applyBackup(context, bytes, passphrase)
            }
        }
        message = outcome.fold(
            onSuccess = { (r, address) ->
                DucatLog.i("Backup", "imported: ${r.contacts.size} contact(s), " +
                    "${r.prekeyOneTime.size} prekey(s), escrow ${r.escrowCount}")
                restored = address
                context.getString(R.string.backup_opened,
                    r.escrowCount.toInt(), r.restoreHeight.toLong())
            },
            onFailure = { t ->
                // A wrong passphrase and a tampered file are the same error, on
                // purpose: telling them apart would say whether a guess was
                // close. The class name goes to the log so that a failure which
                // is *neither* — the cancellation above, an unreadable file —
                // can still be told apart by whoever reads it.
                DucatLog.w("Backup", "import failed: ${t.javaClass.simpleName}: ${t.message}")
                context.getString(R.string.backup_could_not_open)
            },
        )
        busy = false
        pendingImport = null
    }
}

/**
 * Open a bundle and become what is in it. Returns it, and the address its
 * wallet controls.
 *
 * One implementation for both doors — setup and settings — because a restore
 * that puts back different things depending on where it was started from is
 * two features wearing one name, and the parts that were missing here were
 * missing quietly.
 *
 * Blocking and order-sensitive: call it off the main thread, and inside
 * `NonCancellable`. It writes several stores, and interrupted halfway it
 * leaves a device that is neither the old identity nor the new one.
 */
internal fun applyBackup(
    context: Context,
    bytes: ByteArray,
    passphrase: String,
): Pair<uniffi.ducat_mobile.RestoredBackup, String> {
    val r = importBackup(bytes, passphrase)
    // Who this device *is*. Contacts are keyed by their persona, but what we
    // send is signed with ours, so a device that recovered the threads and
    // kept its own keypair is a stranger to everyone in them.
    PersonaStore(context).restoreSecret(r.personaSecret)
    PersonaStore(context).restoreRoster(r.personas)
    // Settings come back too. A restore that keeps the money and drops the name
    // and the privacy choice has quietly changed both, and the user has no way
    // to notice — publishing especially, where the wrong direction is a silent
    // disclosure.
    r.displayName?.let { NameStore(context).put(it) }
    ContactStore(context).setPublishAddress(r.publishPayto)
    // §16.9's profile with it. A persona that comes back with the right money
    // and no face is not the same person to anyone who knew them, and nothing
    // else in the app would report the loss.
    MyProfile(context).let { p ->
        p.setAvatar(r.profile.avatar)
        p.setEmail(r.profile.email)
        p.setPhone(r.profile.phone)
        p.setSignal(r.profile.signal)
        p.setPronouns(r.profile.pronouns?.toInt())
    }
    // The relationships. Threads and tabs from the opaque blob, then the typed
    // contacts as the authoritative overlay — so a bundle from another client
    // still restores everyone.
    ContactStore(context).restoreFromBackup(r)
    // What this device advertises is now a lie. The one-time secrets come back
    // as they were when the bundle was written, but the ids burned between then
    // and the export are dropped — and the peers still hold the older offer
    // listing them, with no memory of which they already spent, because that
    // ledger is not in a backup either. So they seal to keys this device cannot
    // open, and every message arrives unreadable until it happens to send one.
    // Recutting is the repair, and it already exists; it just waited for an
    // outgoing message. A phone restored after a loss receives first — someone
    // checking they are back — so the wait is exactly the wrong way round.
    ContactStore(context).setBundlesNeedRepublish(true)
    // §4.3.3's escrows, before the wallet — an open escrow is money that needs
    // two signatures to move, and on the two-party rung this share is one of
    // exactly two in existence. Nothing else in a bundle is unrecoverable by
    // the people still holding their own copies; this is.
    Ceremony.restoreShares(context, r.escrowShares)
    // The money. The bundle carries the spend key and the height that key was
    // born at; before this, the import applied the contacts and dropped the
    // wallet on the floor.
    val address = addressForSpendKey(r.spendKeyHex, stagenet = true)
    val wallet = WalletStore(context)
    wallet.save(address, r.spendKeyHex, r.restoreHeight, stagenet = true)
    // The outputs belong to the key being replaced, and so does the scan
    // progress. Left alone, `scannedTo` stays at the tip the old wallet
    // reached, so the scanner never looks at the range where this one's money
    // is — a restore that reports success and then finds nothing, forever.
    wallet.rescanFrom(r.restoreHeight.toLong())
    // The bundle just opened *is* a current backup of this device, so say so.
    // Otherwise the first thing a restored phone does is tell the person who
    // has only ever used a backup that they do not have one — "you have
    // contacts your last backup does not", over the file they are holding.
    ContactStore(context).markBackupExported()
    // The address is the check that matters. A bundle that decrypts has proved
    // the passphrase, not that it holds the wallet you meant.
    return r to address
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

/**
 * What a passphrase is actually worth, said plainly.
 *
 * Shared by the two screens that ask for one — setup and settings — because a
 * rule stated differently in two places is a rule somebody disagrees with.
 */
@Composable
internal fun PassphraseNote(passphrase: String) {
    val s = remember(passphrase) {
        uniffi.ducat_mobile.passphraseStrength(passphrase)
    }
    val (text, colour) = when (s) {
        uniffi.ducat_mobile.PassphraseStrength.TOO_SHORT ->
            stringResource(R.string.onb_backup_passphrase_short) to
                MaterialTheme.colorScheme.onSurfaceVariant
        uniffi.ducat_mobile.PassphraseStrength.WEAK ->
            stringResource(R.string.onb_backup_passphrase_weak) to
                MaterialTheme.ducat.lowCapacity
        uniffi.ducat_mobile.PassphraseStrength.FAIR ->
            stringResource(R.string.onb_backup_passphrase_fair) to
                MaterialTheme.colorScheme.onSurfaceVariant
        uniffi.ducat_mobile.PassphraseStrength.STRONG ->
            stringResource(R.string.onb_backup_passphrase_good) to
                MaterialTheme.ducat.settled
    }
    Text(text, color = colour)
}
