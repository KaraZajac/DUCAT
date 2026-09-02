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
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import org.ducatproject.ducat.R
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import java.io.File
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
    // Saveable, as setup's is: a rotation rebuilt this card, and a
    // passphrase chosen with care — and the "Exported N bytes" that said
    // the last tap worked — were gone with no sign they had been there.
    var passphrase by rememberSaveable { mutableStateOf("") }
    var message by rememberSaveable { mutableStateOf<String?>(null) }
    var restored by remember { mutableStateOf<String?>(null) }
    var pendingImport by remember { mutableStateOf<Uri?>(null) }
    // Export encrypts and writes a file; import decrypts one. Both are heavy
    // enough to jank the frame if run on the main thread, and both used to,
    // with no sign the tap had landed. One flag disables both and shows a
    // spinner so a slow encrypt reads as working, not frozen.
    var importing by remember { mutableStateOf(ThreadSends.inFlight(RestoreRun.KEY)) }
    // The export runs off the screen, under the key setup's export uses —
    // see [BACKUP_EXPORT_KEY]. It used to run in this screen's scope, and
    // a rotation or a call landing while Argon2id ground took the scope
    // with it: the file was written and marked, and the share sheet — the
    // only reason to export from here — never opened. Whoever is up when
    // it lands opens it.
    var exporting by remember { mutableStateOf(ThreadSends.inFlight(BACKUP_EXPORT_KEY)) }
    val busy = importing || exporting
    val tick by ThreadSends.ticks.collectAsState()
    LaunchedEffect(tick) {
        importing = ThreadSends.inFlight(RestoreRun.KEY)
        for (o in ThreadSends.take(RestoreRun.KEY)) when (o) {
            is ThreadSends.Outcome.Landed -> {
                restored = RestoreRun.landed?.second
                message = o.result
            }
            is ThreadSends.Outcome.Failed -> {
                // A wrong passphrase and a tampered file are the same error,
                // on purpose: telling them apart would say whether a guess was
                // close. The class name goes to the log so that a failure
                // which is *neither* — an unreadable file, no room — can still
                // be told apart by whoever reads it.
                DucatLog.w(
                    "Backup",
                    "import failed: ${o.error.javaClass.simpleName}: ${o.error.message}",
                )
                message = context.getString(R.string.backup_could_not_open)
            }
        }
        exporting = ThreadSends.inFlight(BACKUP_EXPORT_KEY)
        for (o in ThreadSends.take(BACKUP_EXPORT_KEY)) when (o) {
            is ThreadSends.Outcome.Landed -> {
                // The share sheet is an activity; it has to start on the
                // main thread, which this effect is.
                val f = File(o.result!!)
                share(context, f)
                message = context.getString(R.string.backup_exported_bytes, f.length())
            }
            is ThreadSends.Outcome.Failed -> {
                DucatLog.w("Backup", "export: ${o.error.javaClass.simpleName}: ${o.error.message}")
                message = o.error.saidWhy() ?: context.getString(R.string.backup_export_failed)
            }
        }
    }

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
                        exporting = true; message = null
                        ThreadSends.launch(ContactStore(context), BACKUP_EXPORT_KEY, null) {
                            val bytes = exportBackup(
                                BackupInput(
                                    spendKeyHex!!,
                                    restoreHeight,
                                    // The user's own settings travel with
                                    // their keys: a restore that keeps the
                                    // money and loses the name and the
                                    // privacy choice quietly changed both.
                                    NameStore(
                                        context,
                                        PersonaStore(context).personaHex(),
                                    ).get(),
                                    ContactStore(context).publishAddress(),
                                    MyProfile(
                                        context,
                                        PersonaStore(context).personaHex(),
                                    ).toWire(),
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
                                    PersonaStore(context).backupPersonas(context),
                                ),
                                passphrase,
                                personaSecret!!,
                            )
                            val f = setupBackupFile(context)
                            f.parentFile?.mkdirs()
                            f.writeBytes(bytes)
                            ContactStore(context).markBackupExported()
                            f.absolutePath
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

    // The import runs off this screen, under the same key setup's restore
    // uses — it is the same act, and two at once would be a disaster. The
    // effect only hands it over, so `pendingImport` can be cleared at once:
    // it was cleared last, because clearing it changed this effect's key and
    // cancelled the restore mid-write, which reached the user as "wrong
    // passphrase, or the file has been altered" — a lie about their backup
    // at the worst possible moment. What replaced that fix is stronger:
    // nothing this screen does can cancel the work at all.
    LaunchedEffect(pendingImport) {
        val uri = pendingImport ?: return@LaunchedEffect
        val phrase = passphrase
        importing = true
        ThreadSends.launch(ContactStore(context), RestoreRun.KEY, null) {
            val bytes = context.contentResolver.openInputStream(uri)!!.use { it.readBytes() }
            val landed = applyBackup(context, bytes, phrase)
            RestoreRun.landed = landed
            val (r, _) = landed
            DucatLog.i("Backup", "imported: ${r.contacts.size} contact(s), " +
                "${r.prekeyOneTime.size} prekey(s), escrow ${r.escrowCount}")
            context.getString(R.string.backup_opened,
                r.escrowCount.toInt(), r.restoreHeight.toLong())
        }
        pendingImport = null
    }
}

/**
 * The import that is running, and what it found, held by the process.
 *
 * `applyBackup` is uncancellable — it has to be, since it rewrites this
 * device's keys and stores mid-way — but the line after it that shows the
 * result was not. A turn of the phone while Argon2 ground therefore put the
 * whole backup on the device and then drew the "pick a file" card over it
 * again, with Cancel live: and Cancel from there goes back to "Create a
 * persona", which offers to mint a fresh identity on top of a wallet that
 * has just been restored. On the one screen somebody only reaches by having
 * already lost a phone.
 *
 * ThreadSends carries whether it is running and whether it failed; this
 * carries the answer itself, which is a uniffi object and not a string.
 */
internal object RestoreRun {
    const val KEY = "restore:import"

    @Volatile
    var landed: Pair<uniffi.ducat_mobile.RestoredBackup, String>? = null
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
 * Blocking and order-sensitive: call it off the main thread, and somewhere
 * no screen can cancel it — both doors hand it to [ThreadSends] under
 * [RestoreRun.KEY]. It writes several stores, and interrupted halfway it
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
    // Settings come back too. A restore that keeps the money and drops the name
    // and the privacy choice has quietly changed both, and the user has no way
    // to notice — publishing especially, where the wrong direction is a silent
    // disclosure.
    r.displayName?.let { NameStore(context).put(it) }
    ContactStore(context).setPublishAddress(r.publishPayto)
    // §16.9's profile with it. A persona that comes back with the right money
    // and no face is not the same person to anyone who knew them, and nothing
    // else in the app would report the loss.
    MyProfile(context, PersonaStore(context).personaHex()).let { p ->
        p.setAvatar(r.profile.avatar)
        p.setEmail(r.profile.email)
        p.setPhone(r.profile.phone)
        p.setSignal(r.profile.signal)
        p.setPronouns(r.profile.pronouns?.toInt())
    }
    // The roster after the legacy fields: each entry that carries its own
    // profile overlays them, so a new bundle dresses every hat and an old
    // one leaves the primary's top-level restore standing.
    PersonaStore(context).restoreRoster(context, r.personas)
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
        // **With ClipData, not only the extra.** Android grants the read
        // through whichever of the two it finds, and the share sheet builds
        // its preview from ClipData alone — without it the receiving app can
        // be handed a URI it may not open, over a sheet showing a blank tile.
        // The phone says so in the log every time ("call Intent#setClipData
        // to ensure that the sharesheet is given permission"), which is where
        // this was found: opening a publication on a phone with no viewer for
        // it, watching the sheet come up empty.
        clipData = android.content.ClipData.newUri(context.contentResolver, file.name, uri)
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
