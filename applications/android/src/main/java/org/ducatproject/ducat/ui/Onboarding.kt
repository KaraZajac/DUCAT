package org.ducatproject.ducat.ui

import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
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
import org.ducatproject.ducat.Ceremony
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.NameStore
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.R
import org.ducatproject.ducat.Stakes
import org.ducatproject.ducat.WalletStore
import org.ducatproject.ducat.saidWhy
import uniffi.ducat_mobile.BackupInput
import uniffi.ducat_mobile.NewWallet
import uniffi.ducat_mobile.createPersonaSecret
import uniffi.ducat_mobile.createWallet
import uniffi.ducat_mobile.exportBackup

/**
 * Six steps, and the order is an argument rather than a sequence.
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
enum class Step { Persona, Profile, Wallet, Pin, Trust, Backup, Done }

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
     *
     * "The setup screen states it plainly" is the whole of that argument, and
     * for a year of builds it did not: [Step.Profile] existed, with its
     * switch and both explanations, and nothing ever went there — Persona
     * stepped straight to Wallet, and Finish wrote this default into the
     * store. Every card a new user handed out carried a reused address
     * nobody had chosen and nobody had been told about, which is the exact
     * outcome the reversal was argued to be safe from. Persona goes to
     * Profile now.
     */
    val publishPayto: Boolean = true,
    /** §16.9's optional profile, gathered at setup and editable afterwards. */
    val profile: uniffi.ducat_mobile.Profile =
        uniffi.ducat_mobile.Profile(null, null, null, null, null, null, null, null),
    val backupConfirmed: Boolean = false,
) {
    companion object {
        /**
         * What a rotation keeps: the step, and the answers given so far.
         *
         * MainActivity rebuilds this from the stores when it has nothing
         * saved, and that is the right answer after the process has been
         * killed — but it was the *only* answer, so a phone turned on its
         * side at the Profile step went back to "Create your identity",
         * the name typed so far and the switch's position with it. The
         * stores cannot tell a step apart from the one before it until the
         * step is answered; the saved state can. The profile is not carried:
         * nothing here writes it yet.
         */
        val Saver: Saver<Onboarding, Any> = listSaver(
            save = {
                listOf(it.step.name, it.displayName ?: "", it.publishPayto, it.backupConfirmed)
            },
            restore = {
                Onboarding(
                    step = Step.valueOf(it[0] as String),
                    displayName = (it[1] as String).ifEmpty { null },
                    publishPayto = it[2] as Boolean,
                    backupConfirmed = it[3] as Boolean,
                )
            },
        )
    }
}

@Composable
fun OnboardingFlow(state: Onboarding, onState: (Onboarding) -> Unit) {
    val context = LocalContext.current
    // Saveable, because the restore screen hands off to the system's file
    // picker, and a phone turned in the picker recreates this activity
    // underneath it. Plain `remember` forgot the flag, the recreated flow
    // drew the Persona card, and the file the picker came back with was
    // delivered to a launcher nothing was composing any more — one more
    // silent nothing for somebody who has already lost a phone.
    //
    // Only honoured while the flow is still at its first step. A restore
    // that has already landed resumes at the PIN (MainActivity reads the
    // wallet it put in place), and drawing the restore form over that would
    // offer to import again the file that just was.
    var restoringFlag by rememberSaveable { mutableStateOf(false) }
    val restoring = restoringFlag && state.step == Step.Persona
    Column(
        // imePadding before the scroll: with the keyboard up the column
        // shrinks to the space above it and becomes scrollable, instead of
        // painting the backup step's Done button underneath the keys — where
        // a first-run user, on the one screen they cannot skip, watched a
        // "stuck" page (found on a fresh device, 2026-08-28).
        Modifier.fillMaxSize().imePadding()
            .verticalScroll(rememberScrollState()).padding(24.dp),
    ) {
        // A restore is a shorter flow and has to be counted as one — see below.
        Progress(state.step, restoring || state.backupConfirmed)
        Spacer(Modifier.height(24.dp))

        if (restoring) {
            RestoreStep(
                onCancel = { restoringFlag = false },
                onRestored = { r ->
                    // Past the backup step: the file that brought them here is
                    // the backup, and asking for a second one before letting
                    // this person in would be ceremony, not safety. Past the
                    // trust explainer too — they have used this before.
                    //
                    // **Not past the PIN.** That jumped straight to Done, and
                    // the PIN is the one thing in setup a backup cannot carry:
                    // it is device-local by design, so a restored phone had
                    // none. The step's own docstring says why that matters —
                    // it is what stands between somebody who picks up an
                    // unlocked phone and the money on it, and asking for it
                    // later means asking somebody who is holding a customer —
                    // and later is exactly where it ended up, at the first
                    // payment, from a gate that offers to set one because the
                    // alternative is locking an owner out.
                    //
                    // So: restore, then choose a PIN, then done. One screen,
                    // on the device that now has the money on it.
                    // Out of the restore branch first.
                    //
                    // `restoring` gates everything below it and returns early,
                    // so advancing the step without lowering this flag changed
                    // a value nothing was reading: the screen went on drawing
                    // "Restored wallet" and its Continue button, which
                    // recomputed the same state and drew it again. A working
                    // restore — right wallet, right address — that could not
                    // be walked away from, on the one path somebody reaches by
                    // having already lost a phone.
                    restoringFlag = false
                    onState(
                        state.copy(
                            step = Step.Pin,
                            backupConfirmed = true,
                            displayName = r.displayName,
                            publishPayto = r.publishPayto,
                        ),
                    )
                },
            )
            return@Column
        }

        when (state.step) {
            Step.Persona -> StepCard(
                title = stringResource(R.string.onb_persona_title),
                body = stringResource(R.string.onb_persona_body),
                action = stringResource(R.string.onb_persona_action),
                secondary = stringResource(R.string.onb_have_backup),
                onSecondary = { restoringFlag = true },
                onAction = {
                    // Persisted the moment it is created, not held in Compose
                    // state until Done. A rotation used to throw the persona
                    // away and mint a fresh one at Finish — so the identity the
                    // backup was signed with was not the identity the app then
                    // ran under. secret() writes it once and returns the same
                    // bytes on every later read.
                    PersonaStore(context).secret()
                    onState(state.copy(step = Step.Profile))
                },
            )

            Step.Profile -> ProfileStep(
                initialName = state.displayName.orEmpty(),
                initialPublish = state.publishPayto,
                onNext = { name, publish ->
                    // Written here, under the same rule as the persona above
                    // and the wallet below: a rotation rebuilds this flow from
                    // the stores, and anything only Compose held is gone. Held
                    // until Done, the name went missing on the first rotation
                    // and the privacy answer reverted to the default — which,
                    // for somebody who had just read why and said no, is the
                    // one answer they had ruled out.
                    val persona = PersonaStore(context).personaHex()
                    if (name.isNotBlank()) NameStore(context, persona).put(name)
                    ContactStore(context).setPublishAddress(publish)
                    onState(
                        state.copy(
                            step = Step.Wallet,
                            displayName = name.ifBlank { null },
                            publishPayto = publish,
                        )
                    )
                },
            )

            Step.Wallet -> {
                // Minting takes a network round trip and a key derivation;
                // both ran on the tap itself, so the button froze the screen
                // for up to the node timeout — eight seconds of an app that
                // had apparently hung on its second step. Off the main
                // thread, with the button held down while it runs.
                val scope = rememberCoroutineScope()
                var minting by remember { mutableStateOf(false) }
                var failed by remember { mutableStateOf(false) }
                // A wallet already in place means this step's work is done —
                // a rotation while it was minting cancelled the scope above
                // after the store was written, and the rebuilt card offered
                // to create what exists. Same rule MainActivity resumes by.
                LaunchedEffect(Unit) {
                    if (withContext(Dispatchers.IO) { WalletStore(context).address() } != null) {
                        onState(state.copy(step = Step.Pin))
                    }
                }
                StepCard(
                    title = stringResource(R.string.onb_wallet_title),
                    body = stringResource(R.string.onb_wallet_body),
                    action = stringResource(R.string.onb_wallet_action),
                    busy = minting,
                    // Said, not only logged. The failure went to the log and
                    // the button came back as if nothing had been tapped —
                    // on the one step a new user cannot go around.
                    error = if (failed) stringResource(R.string.onb_wallet_error) else null,
                    onAction = {
                        if (!minting) {
                            minting = true
                            failed = false
                            scope.launch {
                                val made = runCatching {
                                    withContext(Dispatchers.IO) {
                                        // A wallet already in the store is not
                                        // this step's to replace. The only way
                                        // one is there before this mints it is
                                        // a restore that landed under a
                                        // rotation — its import runs on past
                                        // the screen that started it — and
                                        // minting over it would throw away the
                                        // money the restore had just brought
                                        // back.
                                        if (WalletStore(context).address() != null) {
                                            return@withContext
                                        }
                                        // Genesis until a node supplies a real
                                        // tip — slow to restore, and
                                        // recoverable, which is the side of
                                        // §4.3.1's asymmetry to be on.
                                        // Ask a node where the chain is first.
                                        // A wallet that does not know its own
                                        // creation height scans from genesis,
                                        // which is a day and a half of reading
                                        // that looks exactly like having no
                                        // money.
                                        val tip = runCatching {
                                            uniffi.ducat_mobile.moneroPickNode(
                                                uniffi.ducat_mobile.moneroDefaultNodes(null),
                                                "stagenet",
                                                8000u,
                                            ).height
                                        }.getOrDefault(UNKNOWN_TIP)
                                        // Persisted at creation, so a rotation
                                        // cannot regenerate a *different*
                                        // wallet than the address the user was
                                        // shown and the backup they wrote.
                                        // onboarded stays false until Backup,
                                        // so this does not open a funded
                                        // wallet before §4.3's step.
                                        val w = createWallet(tipHeight = tip, stagenet = true)
                                        WalletStore(context).save(
                                            address = w.address, spendKeyHex = w.spendKeyHex,
                                            restoreHeight = w.restoreHeight, stagenet = true,
                                        )
                                    }
                                }
                                minting = false
                                made.onSuccess { onState(state.copy(step = Step.Pin)) }
                                    .onFailure {
                                        org.ducatproject.ducat.DucatLog.w(
                                            "Onboarding",
                                            "wallet: ${it.javaClass.simpleName}: ${it.message}",
                                        )
                                        failed = true
                                    }
                            }
                        }
                    },
                )
            }

            // This step used to describe a PIN the app did not have —
            // "larger payments ask for your PIN", with nothing behind it.
            // Now it is where the PIN is actually chosen, before there is any
            // money to lose and while a person is still paying attention to
            // set-up rather than to a customer.
            // Trust and Backup after this for a new wallet; straight to Done
            // for a restored one, which has already done both.
            Step.Pin -> PinStep(
                onDone = {
                    onState(
                        state.copy(
                            step = if (state.backupConfirmed) Step.Done else Step.Trust,
                        ),
                    )
                },
            )

            // Before the first deal, because it is the answer to the
            // question every user of a marketplace without a company asks
            // sooner or later: what stops the other person from cheating me?
            Step.Trust -> StepCard(
                title = stringResource(R.string.onb_trust_title),
                body = stringResource(
                    R.string.onb_trust_body,
                    Stakes.Deal.Ride.percent,
                    Stakes.Deal.Stay.percent,
                    Stakes.Deal.Vehicle.percent,
                ),
                action = stringResource(R.string.onb_trust_action),
                onAction = { onState(state.copy(step = Step.Backup)) },
            )

            // Deliberately before funding, and deliberately not skippable.
            Step.Backup -> BackupStep(
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
private fun Progress(step: Step, restored: Boolean) {
    // The step you are *on*, not the number completed. "0 of 4" on the first
    // screen reads as though nothing has started and something has gone wrong.
    //
    // The flow is six steps — Persona → Profile → Wallet → PIN → Trust →
    // Backup — then Done, numbered in the order they are shown, which is
    // what keeps "Step 2 of 6" from ever jumping to "Step 4".
    //
    // A restore is a different, shorter flow: the file that brought them here
    // is the backup, so the wallet, the trust explainer and the backup step
    // are all behind them, and what is left is the restore itself and a PIN.
    // Counting that against six said "Step 3 of 6" and then finished — a bar
    // that leapt from half to full, to somebody who has just lost a phone
    // and is watching this screen closely.
    val total = if (restored) 2 else 6
    val n = if (restored) {
        when (step) {
            Step.Pin -> 2
            Step.Done -> 2
            else -> 1 // the restore screen itself
        }
    } else when (step) {
        Step.Persona -> 1
        Step.Profile -> 2
        Step.Wallet -> 3
        Step.Pin -> 4
        Step.Trust -> 5
        Step.Backup -> 6
        Step.Done -> total
    }
    Column {
        Text(stringResource(R.string.onb_progress_title), style = MaterialTheme.typography.headlineSmall)
        Spacer(Modifier.height(8.dp))
        DucatBar(progress = n.toFloat() / total)
        Spacer(Modifier.height(4.dp))
        Text(
            if (step == Step.Done) stringResource(R.string.onb_progress_done)
            else stringResource(R.string.onb_progress_step, n, total),
            style = MaterialTheme.typography.bodySmall,
        )
    }
}

/**
 * Choose the PIN, at the one moment a person is set up to think about it.
 *
 * Not skippable, and deliberately so: every other step here builds something
 * that can be rebuilt — a name, a profile, a wallet that a backup restores.
 * This is the only one that stands between somebody who picks up an unlocked
 * phone and the money on it, and asking for it later means asking somebody
 * who is holding a customer.
 */
@Composable
private fun PinStep(onDone: () -> Unit) {
    var asking by remember { mutableStateOf(false) }
    val context = LocalContext.current
    var done by remember { mutableStateOf(org.ducatproject.ducat.Pin.isSet(context)) }
    StepCard(
        title = stringResource(R.string.onb_pin_title),
        body = stringResource(if (done) R.string.onb_pin_done else R.string.onb_pin_body),
        action = stringResource(if (done) R.string.onb_pin_next else R.string.onb_pin_action),
        onAction = { if (done) onDone() else asking = true },
    )
    PinGate(
        open = asking,
        onDismiss = { asking = false },
        onPassed = { asking = false; done = true },
    )
}

@Composable
private fun StepCard(
    title: String,
    body: String,
    action: String,
    onAction: () -> Unit,
    secondary: String? = null,
    onSecondary: (() -> Unit)? = null,
    /** The action is under way: the button waits rather than fires twice. */
    busy: Boolean = false,
    /** What the last attempt at the action said, when it failed. */
    error: String? = null,
) {
    Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(16.dp)) {
        Column(Modifier.padding(20.dp)) {
            Text(title, style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(8.dp))
            Text(body, style = MaterialTheme.typography.bodyMedium)
            Spacer(Modifier.height(16.dp))
            Button(onClick = onAction, enabled = !busy) {
                if (busy) {
                    CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                    Spacer(Modifier.width(8.dp))
                }
                Text(action)
            }
            error?.let {
                Spacer(Modifier.height(8.dp))
                Text(
                    it,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
            }
            if (secondary != null && onSecondary != null) {
                TextButton(onClick = onSecondary) { Text(secondary) }
            }
        }
    }
}

/**
 * The other way in: this phone is not new, the person is.
 *
 * Setup used to offer creating an identity and nothing else, so somebody
 * holding the encrypted file from a lost phone had to make a *fresh* identity
 * and wallet, back **that** up to get past the last step, and only then find
 * Settings → Backup → Import. The one moment restoring is the whole reason
 * they opened the app was the one place it was not offered.
 *
 * It restores before anything is minted, so no throwaway keypair is created
 * and no empty wallet is left behind. The address is shown before the door
 * opens, for the same reason the settings screen shows it: a bundle that
 * decrypts has proved the passphrase, not that it holds the wallet you meant.
 */
@Composable
private fun RestoreStep(
    onCancel: () -> Unit,
    onRestored: (uniffi.ducat_mobile.RestoredBackup) -> Unit,
) {
    val context = LocalContext.current
    // Saveable for the same reason as the flag that shows this card: the
    // picker's result comes back to a recreated activity, and the import it
    // starts needs the passphrase typed before the picker was opened. With
    // it forgotten, the file that came back was tried against "" and
    // reported as the wrong passphrase.
    var passphrase by rememberSaveable { mutableStateOf("") }
    var pending by remember { mutableStateOf<android.net.Uri?>(null) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var done by remember {
        mutableStateOf<Pair<uniffi.ducat_mobile.RestoredBackup, String>?>(null)
    }

    val picker = rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.OpenDocument()
    ) { uri -> pending = uri }

    // The Cancel button below, reachable by gesture.
    //
    // A restore is a second level: the flow returns early on `restoring`, so
    // the step behind this card is not on screen and there is nothing under it
    // to fall through to. With no handler the press went past the whole of
    // setup to the activity and closed the app, on the one screen somebody
    // only reaches by having already lost a phone.
    //
    // It follows the button rather than the flag, in both directions the
    // button is unavailable. Mid-import there is nothing to cancel — the work
    // is NonCancellable for the reason the effect above gives — and once the
    // backup is *in*, the wallet is on this device: going back to "Create a
    // persona" would offer to mint a fresh one over the top of it, and the
    // only honest way on is Continue.
    BackHandler { if (!busy && done == null) onCancel() }

    // `pending` is cleared last and the work is NonCancellable — clearing it
    // first would change this effect's key and cancel the restore it started.
    LaunchedEffect(pending) {
        val uri = pending ?: return@LaunchedEffect
        busy = true
        error = null
        val outcome = withContext(Dispatchers.IO + kotlinx.coroutines.NonCancellable) {
            runCatching {
                val bytes = context.contentResolver.openInputStream(uri)!!
                    .use { it.readBytes() }
                applyBackup(context, bytes, passphrase)
            }
        }
        outcome.onSuccess { done = it }.onFailure {
            org.ducatproject.ducat.DucatLog.w(
                "Backup", "setup restore: ${it.javaClass.simpleName}: ${it.message}",
            )
            error = context.getString(R.string.backup_could_not_open)
        }
        busy = false
        pending = null
    }

    Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(16.dp)) {
        Column(Modifier.padding(20.dp)) {
            Text(
                stringResource(R.string.onb_restore_title),
                style = MaterialTheme.typography.titleLarge,
            )
            Spacer(Modifier.height(8.dp))
            Text(
                stringResource(R.string.onb_restore_body),
                style = MaterialTheme.typography.bodyMedium,
            )

            val restored = done
            if (restored == null) {
                Spacer(Modifier.height(16.dp))
                OutlinedTextField(
                    value = passphrase,
                    onValueChange = { passphrase = it; error = null },
                    label = { Text(stringResource(R.string.backup_passphrase)) },
                    singleLine = true,
                    enabled = !busy,
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(12.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Button(
                        enabled = !busy && passphrase.length >= 8,
                        onClick = { picker.launch(arrayOf("*/*")) },
                    ) {
                        if (busy) {
                            CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                        } else {
                            Text(stringResource(R.string.onb_restore_pick))
                        }
                    }
                    Spacer(Modifier.width(8.dp))
                    TextButton(enabled = !busy, onClick = onCancel) {
                        Text(stringResource(R.string.onb_restore_cancel))
                    }
                }
                error?.let {
                    Spacer(Modifier.height(8.dp))
                    Text(
                        it,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error,
                    )
                }
            } else {
                Spacer(Modifier.height(16.dp))
                Text(
                    stringResource(R.string.backup_restored_title),
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    stringResource(R.string.backup_restored_check),
                    style = MaterialTheme.typography.bodySmall,
                )
                Text(
                    restored.second,
                    style = MaterialTheme.typography.bodySmall,
                    fontFamily = FontFamily.Monospace,
                )
                Spacer(Modifier.height(16.dp))
                Button(onClick = { onRestored(restored.first) }) {
                    Text(stringResource(R.string.onb_restore_continue))
                }
            }
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
private fun BackupStep(onDone: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    // Saveable for the same reason the restore form's is: a rotation
    // rebuilt this card, and eight or more characters somebody had chosen
    // with care were gone from the field with no sign they had been typed.
    var passphrase by rememberSaveable { mutableStateOf("") }
    // Starts on the file if setup already wrote one. The write marks the
    // backup as taken the moment it lands, and MainActivity resumes a flow
    // from that mark — so a phone turned on the "Backup created" card
    // rebuilt this step and, until this read, found nothing written: the
    // bundle sat in app-private storage with the row that offers to send it
    // somewhere never shown again. The one file that matters, kept where
    // losing the phone loses it.
    var written by remember {
        mutableStateOf(
            setupBackupFile(context)
                .takeIf { ContactStore(context).backupExportedAt() > 0L && it.exists() },
        )
    }
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
                // Graded rather than measured — see `PassphraseNote`. The
                // button below still gates on the floor; this says what
                // clearing it is actually worth.
                supportingText = { PassphraseNote(passphrase) },
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
                                            // The stores, as Settings' export
                                            // reads them, and not the flow's
                                            // state: the Profile step writes
                                            // both as it is answered, and the
                                            // state is rebuilt from the stores
                                            // on every rotation anyway.
                                            NameStore(context, PersonaStore(context).personaHex()).get(),
                                            ContactStore(context).publishAddress(),
                                            MyProfile(context, PersonaStore(context).personaHex()).toWire(),
                                            // Read, not assumed empty. "First run:
                                            // no relationships yet" was true of the
                                            // only path this screen had when it was
                                            // written, and stopped being true when
                                            // restoring gained one: step 5 is where
                                            // a restore ends too, and it was handing
                                            // that person a bundle with no contacts,
                                            // no prekeys, no threads and no escrows —
                                            // 215 bytes against the 51 KB file they
                                            // had just opened, offered as their
                                            // backup. On a genuine first run these
                                            // stores are empty and this reads the
                                            // same emptiness the constants asserted.
                                            ContactStore(context).backupContacts(),
                                            ContactStore(context).backupPrekeys().first,
                                            ContactStore(context).backupPrekeys().second,
                                            ContactStore(context).backupPrekeys().third.toULong(),
                                            ContactStore(context).backupAppState(),
                                            Ceremony.backupShares(context),
                                            PersonaStore(context).backupPersonas(context),
                                        ),
                                        passphrase,
                                        persona,
                                    )
                                    val f = setupBackupFile(context)
                                    f.parentFile?.mkdirs()
                                    f.writeBytes(bytes)
                                    // Say that one exists. Settings' export has
                                    // always recorded this and setup's never did,
                                    // so the app finished setup believing the
                                    // backup it had just written did not exist —
                                    // which is the baseline every later "your
                                    // backup is out of date" is measured against,
                                    // and the answer to what is missing when a
                                    // killed process resumes mid-setup.
                                    ContactStore(context).markBackupExported()
                                    f
                                }
                            }
                            result.onSuccess { written = it }
                                .onFailure {
                                    // Settings' export says the same on the
                                    // same failure. This said the exception's
                                    // class name — "IOException" in every
                                    // language.
                                    org.ducatproject.ducat.DucatLog.w(
                                        "Onboarding",
                                        "backup: ${it.javaClass.simpleName}: ${it.message}",
                                    )
                                    error = it.saidWhy()
                                        ?: context.getString(R.string.backup_export_failed)
                                }
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
 * Where setup writes the bundle, and where Settings' export writes its own:
 * app-private, so it goes when the app does, which is why the step that
 * writes it does not end until the person has been offered somewhere else
 * to put it.
 */
internal fun setupBackupFile(context: Context): File =
    File(File(context.filesDir, "backups"), "ducat-backup.ducatbak")

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
    // Saveable: the step survives a rotation now, and a form that did not
    // would have handed back an empty field on it.
    var name by rememberSaveable { mutableStateOf(initialName) }
    var publish by rememberSaveable { mutableStateOf(initialPublish) }

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
                supportingText = { CharCounter(name.length, 32) },
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
