package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import android.content.Context
import android.content.Intent
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.FileProvider
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.R
import org.ducatproject.ducat.WalletStore
import uniffi.ducat_mobile.BackupInput
import uniffi.ducat_mobile.NewWallet
import uniffi.ducat_mobile.createPersonaSecret
import uniffi.ducat_mobile.createWallet
import uniffi.ducat_mobile.exportBackup

/**
 * Four steps, and the order is an argument rather than a sequence.
 *
 * The backup comes **before** the wallet can be funded. It is the step users
 * skip and the only one whose absence is unrecoverable — a persona lost without
 * one takes its reputation and every persistent contact with it, and there is no
 * operator to appeal to, which is the same property that makes the system
 * uncustodied.
 *
 * A user with nothing to lose has no reason to skip it and no reason to resent
 * it. A user with money in the float has both. The one moment when doing it
 * costs nothing is the moment before there is anything to protect.
 */
enum class Step { Persona, Profile, Wallet, Limits, Backup, Done }

/**
 * No node has been asked yet, so the backup records genesis.
 *
 * This was `ULong.MAX_VALUE` for exactly one build, reasoning that zero means
 * genesis and genesis costs ~106 hours of rescan. That reasoning was right about
 * zero and wrong about the alternative: §4.3.1's two directions are **not
 * symmetric**. Too low is slow and recoverable. Too high means a restored wallet
 * scans forward from after every output it owns, finds nothing, and shows a zero
 * balance with no error anywhere.
 *
 * A backup written by the phone carrying 18446744073709551615 was opened on a
 * desktop and proved it. Between an expensive restore and an empty one, take the
 * expensive one.
 */
const val UNKNOWN_TIP: ULong = 0uL

data class Onboarding(
    val step: Step = Step.Persona,
    /** The name handed out on cards (§7.5). Optional, and it can change later. */
    val displayName: String? = null,
    /**
     * Whether contacts may pay without asking (§16.12).
     *
     * **On by default**, and that is a reversal worth naming. It started off
     * because a published address is a reused address, and a reused address is
     * a public ledger entry linking everyone who ever paid this person. What
     * changed is not the cost — the cost is the same — but who is being asked
     * to understand it before they can send a friend twenty. The setup screen
     * states it plainly and the switch is right there; nobody is uninformed,
     * and nobody is stopped at the first step either.
     */
    val publishPayto: Boolean = true,
    /** §16.9's optional profile, gathered at setup and editable afterwards. */
    val profile: uniffi.ducat_mobile.Profile =
        uniffi.ducat_mobile.Profile(null, null, null, null, null, null, null, null),
    val backupConfirmed: Boolean = false,
)

@Composable
fun OnboardingFlow(state: Onboarding, onState: (Onboarding) -> Unit) {
    val context = LocalContext.current
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
    ) {
        Progress(state.step)
        Spacer(Modifier.height(24.dp))

        when (state.step) {
            Step.Persona -> StepCard(
                title = stringResource(R.string.onb_persona_title),
                body = stringResource(R.string.onb_persona_body),
                action = stringResource(R.string.onb_persona_action),
                onAction = {
                    // Persisted the moment it is created, not held in Compose
                    // state until Done. A rotation used to throw the persona
                    // away and mint a fresh one at Finish — so the identity the
                    // backup was signed with was not the identity the app then
                    // ran under. secret() writes it once and returns the same
                    // bytes on every later read.
                    PersonaStore(context).secret()
                    onState(state.copy(step = Step.Wallet))
                },
            )

            Step.Wallet -> StepCard(
                title = stringResource(R.string.onb_wallet_title),
                body = stringResource(R.string.onb_wallet_body),
                action = stringResource(R.string.onb_wallet_action),
                onAction = {
                    // Genesis until a node supplies a real tip — slow to
                    // restore, and recoverable, which is the side of §4.3.1's
                    // asymmetry to be on.
                    // Ask a node where the chain is first. A wallet that does
                    // not know its own creation height scans from genesis, which
                    // is a day and a half of reading that looks exactly like
                    // having no money.
                    val tip = runCatching {
                        uniffi.ducat_mobile.moneroPickNode(
                            uniffi.ducat_mobile.moneroDefaultNodes(null),
                            "stagenet",
                            8000u,
                        ).height
                    }.getOrDefault(UNKNOWN_TIP)
                    // Persisted at creation, so a rotation cannot regenerate a
                    // *different* wallet than the address the user was shown and
                    // the backup they wrote. onboarded stays false until Backup,
                    // so this does not open a funded wallet before §4.3's step.
                    val w = createWallet(tipHeight = tip, stagenet = true)
                    WalletStore(context).save(
                        address = w.address, spendKeyHex = w.spendKeyHex,
                        restoreHeight = w.restoreHeight, stagenet = true,
                    )
                    onState(state.copy(step = Step.Limits))
                },
            )

            Step.Profile -> ProfileStep(
                initialName = state.displayName.orEmpty(),
                initialPublish = state.publishPayto,
                onNext = { name, publish ->
                    onState(
                        state.copy(
                            step = Step.Wallet,
                            displayName = name.ifBlank { null },
                            publishPayto = publish,
                        )
                    )
                },
            )

            Step.Limits -> StepCard(
                title = stringResource(R.string.onb_limits_title),
                body = stringResource(R.string.onb_limits_body),
                action = stringResource(R.string.onb_limits_action),
                onAction = { onState(state.copy(step = Step.Backup)) },
            )

            // Deliberately before funding, and deliberately not skippable.
            Step.Backup -> BackupStep(
                state = state,
                onDone = { onState(state.copy(step = Step.Done, backupConfirmed = true)) },
            )

            Step.Done -> StepCard(
                title = stringResource(R.string.onb_done_title),
                body = stringResource(R.string.onb_done_body),
                action = stringResource(R.string.onb_done_action),
                onAction = { onState(state.copy(step = Step.Done)) },
            )
        }
    }
}

@Composable
private fun Progress(step: Step) {
    // The step you are *on*, not the number completed. "0 of 4" on the first
    // screen reads as though nothing has started and something has gone wrong.
    //
    // The reachable flow is four steps — Persona → Wallet → Limits → Backup —
    // then Done. `Step.Profile` exists but is skipped (Persona goes straight to
    // Wallet), so it is not counted; numbering the shown steps contiguously is
    // what keeps "Step 2 of 4" from ever jumping to "Step 3".
    val total = 4
    val n = when (step) {
        Step.Persona -> 1
        Step.Wallet -> 2
        Step.Profile -> 2 // unreachable in the current flow; kept in range
        Step.Limits -> 3
        Step.Backup -> 4
        Step.Done -> total
    }
    Column {
        Text(stringResource(R.string.onb_progress_title), style = MaterialTheme.typography.headlineSmall)
        Spacer(Modifier.height(8.dp))
        LinearProgressIndicator(
            progress = { n.toFloat() / total },
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(4.dp))
        Text(
            if (step == Step.Done) stringResource(R.string.onb_progress_done)
            else stringResource(R.string.onb_progress_step, n, total),
            style = MaterialTheme.typography.bodySmall,
        )
    }
}

@Composable
private fun StepCard(title: String, body: String, action: String, onAction: () -> Unit) {
    Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(16.dp)) {
        Column(Modifier.padding(20.dp)) {
            Text(title, style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(8.dp))
            Text(body, style = MaterialTheme.typography.bodyMedium)
            Spacer(Modifier.height(16.dp))
            Button(onClick = onAction) { Text(action) }
        }
    }
}

/**
 * The step that cannot be skipped, and the screen that has to say why without
 * sounding like a legal disclaimer nobody reads.
 *
 * §4.3.4 requires this be said in the user's terms rather than in the language of
 * a password reset they might expect to exist: a forgotten passphrase is
 * unrecoverable, and there is no operator to appeal to.
 */
@Composable
private fun BackupStep(state: Onboarding, onDone: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var passphrase by remember { mutableStateOf("") }
    var written by remember { mutableStateOf<File?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val longEnough = passphrase.length >= 8

    Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(16.dp)) {
        Column(Modifier.padding(20.dp)) {
            Text(stringResource(R.string.onb_backup_title), style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(8.dp))
            Text(
                stringResource(R.string.onb_backup_body),
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(12.dp))
            Surface(
                color = MaterialTheme.colorScheme.surfaceVariant,
                shape = RoundedCornerShape(12.dp),
            ) {
                Column(Modifier.padding(12.dp)) {
                    Text(stringResource(R.string.onb_backup_forget_title), fontWeight = FontWeight.SemiBold)
                    Text(
                        stringResource(R.string.onb_backup_forget_body),
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }

            WalletStore(context).address()?.let { addr ->
                Spacer(Modifier.height(12.dp))
                Text(stringResource(R.string.onb_backup_address_label), style = MaterialTheme.typography.labelMedium)
                Text(addr, style = MaterialTheme.typography.bodySmall,
                    fontFamily = FontFamily.Monospace)
            }

            Spacer(Modifier.height(16.dp))
            OutlinedTextField(
                value = passphrase,
                onValueChange = { passphrase = it; error = null },
                label = { Text(stringResource(R.string.onb_backup_passphrase_label)) },
                supportingText = {
                    Text(
                        if (longEnough) stringResource(R.string.onb_backup_passphrase_good)
                        else stringResource(R.string.onb_backup_passphrase_short),
                        color = if (longEnough) MaterialTheme.ducat.settled
                                else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )

            error?.let {
                Spacer(Modifier.height(8.dp))
                Text(it, color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall)
            }

            Spacer(Modifier.height(12.dp))

            if (written == null) {
                // **A file is written before anything is claimed about one.**
                // The first version asked the user to tick "I've saved the file"
                // when no file had ever existed. Someone who ticked it and later
                // lost the phone would have lost everything while believing they
                // were covered — which is worse than having no backup step at
                // all, because it also removes the worry that would have made
                // them do something about it.
                Button(
                    onClick = {
                        // Read the persisted artifacts, not Compose state: a
                        // rotation during onboarding leaves them in the stores,
                        // and this is the same persona the app will run under
                        // and the same wallet its address was shown for.
                        val ws = WalletStore(context)
                        val spendKey = ws.spendKeyHex()
                        if (spendKey == null) {
                            error = context.getString(R.string.onb_backup_error_incomplete)
                            return@Button
                        }
                        val persona = PersonaStore(context).secret()
                        val restoreHeight = ws.restoreHeight()
                        busy = true; error = null
                        scope.launch {
                            // Encrypt and write off the main thread so the setup
                            // screen does not freeze while the file is built.
                            val result = withContext(Dispatchers.IO) {
                                runCatching {
                                    val bytes = exportBackup(
                                        BackupInput(
                                            spendKey,
                                            restoreHeight,
                                            state.displayName,
                                            state.publishPayto,
                                            state.profile,
                                            // First run: no relationships yet.
                                            emptyList(), null, emptyList(), 0uL, null,
                                        ),
                                        passphrase,
                                        persona,
                                    )
                                    val dir = File(context.filesDir, "backups").apply { mkdirs() }
                                    val f = File(dir, "ducat-backup.ducatbak")
                                    f.writeBytes(bytes)
                                    f
                                }
                            }
                            result.onSuccess { written = it }
                                .onFailure { error = it.message ?: it::class.simpleName }
                            busy = false
                        }
                    },
                    enabled = longEnough && !busy,
                ) {
                    if (busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                    else Text(stringResource(R.string.onb_backup_create))
                }
            } else {
                Text(
                    stringResource(R.string.onb_backup_created, written!!.length()),
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    stringResource(R.string.onb_backup_send_elsewhere),
                    style = MaterialTheme.typography.bodySmall,
                )
                Spacer(Modifier.height(8.dp))
                Row {
                    Button(onClick = { shareBackup(context, written!!) }) { Text(stringResource(R.string.onb_backup_send_action)) }
                    Spacer(Modifier.width(8.dp))
                    TextButton(onClick = onDone) { Text(stringResource(R.string.onb_backup_done)) }
                }
            }
        }
    }
}

/**
 * Hand the file to whatever the user already trusts with important things.
 *
 * §4.3 is deliberate that the user chooses where a backup lives — a protocol
 * that also decided *where* would be back to needing a service.
 */
private fun shareBackup(context: Context, file: File) {
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.backups", file)
    val send = Intent(Intent.ACTION_SEND).apply {
        type = "application/octet-stream"
        putExtra(Intent.EXTRA_STREAM, uri)
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    context.startActivity(Intent.createChooser(send, context.getString(R.string.onb_backup_share_chooser)))
}


/**
 * A name, and one decision about being paid.
 *
 * Both belong here rather than buried in settings later. The name is what every
 * card hands out, so a persona created without one introduces itself as a key
 * fragment. And the address question is asked once, before there is anything at
 * stake, rather than at the moment somebody is trying to pay — which is the
 * worst possible time to be reading about linkability.
 *
 * Both are optional and both can change afterwards. Neither is a credential;
 * they travel in the backup because losing them changes how a restored persona
 * behaves, which is not something a restore should do silently.
 */
@Composable
private fun ProfileStep(
    initialName: String,
    initialPublish: Boolean,
    onNext: (String, Boolean) -> Unit,
) {
    var name by remember { mutableStateOf(initialName) }
    var publish by remember { mutableStateOf(initialPublish) }

    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(20.dp)) {
            Text(stringResource(R.string.onb_profile_name_title), style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(8.dp))
            Text(
                stringResource(R.string.onb_profile_name_body),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(14.dp))
            OutlinedTextField(
                value = name,
                onValueChange = { if (it.length <= 32) name = it },
                label = { Text(stringResource(R.string.onb_profile_name_label)) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )

            Spacer(Modifier.height(22.dp))
            Text(stringResource(R.string.onb_profile_pay_title), style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(8.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Switch(checked = publish, onCheckedChange = { publish = it })
                Spacer(Modifier.width(12.dp))
                Text(
                    if (publish) stringResource(R.string.onb_profile_pay_yes)
                    else stringResource(R.string.onb_profile_pay_no),
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
            Spacer(Modifier.height(8.dp))
            Text(
                if (publish) stringResource(R.string.onb_profile_pay_yes_detail)
                else stringResource(R.string.onb_profile_pay_no_detail),
                style = MaterialTheme.typography.bodySmall,
                color = if (publish) MaterialTheme.ducat.changePending
                        else MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            Text(
                stringResource(R.string.onb_profile_change_later),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.outline,
            )

            Spacer(Modifier.height(18.dp))
            Button(
                onClick = { onNext(name.trim(), publish) },
                modifier = Modifier.fillMaxWidth(),
            ) { Text(stringResource(R.string.onb_profile_continue)) }
        }
    }
}
