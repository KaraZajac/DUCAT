package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import android.content.Context
import android.content.Intent
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.FileProvider
import java.io.File
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
    val persona: ByteArray? = null,
    val wallet: NewWallet? = null,
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
        uniffi.ducat_mobile.Profile(null, null, null, null, null),
    val backupConfirmed: Boolean = false,
)

@Composable
fun OnboardingFlow(state: Onboarding, onState: (Onboarding) -> Unit) {
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
    ) {
        Progress(state.step)
        Spacer(Modifier.height(24.dp))

        when (state.step) {
            Step.Persona -> StepCard(
                title = "Create your identity",
                body = "A keypair on this phone. No email, no phone number, nobody to " +
                    "register with — which is also why nobody can restore it for you.",
                action = "Create",
                onAction = {
                    onState(state.copy(step = Step.Wallet, persona = createPersonaSecret()))
                },
            )

            Step.Wallet -> StepCard(
                title = "Create your wallet",
                body = "Monero keys, generated here and held here. This is the money.",
                action = "Create wallet",
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
                    val w = createWallet(tipHeight = tip, stagenet = true)
                    onState(state.copy(step = Step.Limits, wallet = w))
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
                title = "Set your spending limits",
                body = "Small payments go straight through. Larger ones ask for your " +
                    "PIN. You can change where those lines sit at any time — the " +
                    "defaults are already cautious.",
                action = "Keep the defaults",
                onAction = { onState(state.copy(step = Step.Backup)) },
            )

            // Deliberately before funding, and deliberately not skippable.
            Step.Backup -> BackupStep(
                state = state,
                onDone = { onState(state.copy(step = Step.Done, backupConfirmed = true)) },
            )

            Step.Done -> StepCard(
                title = "Ready",
                body = "Add money when you're ready. Until you do, there is nothing at " +
                    "risk and nothing to lose.",
                action = "Finish",
                onAction = { onState(state.copy(step = Step.Done)) },
            )
        }
    }
}

@Composable
private fun Progress(step: Step) {
    // The step you are *on*, not the number completed. "0 of 4" on the first
    // screen reads as though nothing has started and something has gone wrong.
    val n = when (step) {
        Step.Persona -> 1; Step.Profile -> 2; Step.Wallet -> 3; Step.Limits -> 4
        Step.Backup -> 5; Step.Done -> 5
    }
    Column {
        Text("Set up DUCAT", style = MaterialTheme.typography.headlineSmall)
        Spacer(Modifier.height(8.dp))
        LinearProgressIndicator(
            progress = { n / 4f },
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(4.dp))
        Text(
            if (step == Step.Done) "Done" else "Step $n of 5",
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
    var passphrase by remember { mutableStateOf("") }
    var written by remember { mutableStateOf<File?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val longEnough = passphrase.length >= 8

    Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(16.dp)) {
        Column(Modifier.padding(20.dp)) {
            Text("Back it up — now, not later", style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(8.dp))
            Text(
                "One encrypted file, protected by a passphrase you choose. Keep it " +
                    "wherever you keep important things.",
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(12.dp))
            Surface(
                color = MaterialTheme.colorScheme.surfaceVariant,
                shape = RoundedCornerShape(12.dp),
            ) {
                Column(Modifier.padding(12.dp)) {
                    Text("If you forget this passphrase", fontWeight = FontWeight.SemiBold)
                    Text(
                        "there is no way to recover it. Nobody holds a copy and there " +
                            "is no one to ask. That is the same reason nobody can " +
                            "freeze your money or take it.",
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }

            state.wallet?.let { w ->
                Spacer(Modifier.height(12.dp))
                Text("Your address", style = MaterialTheme.typography.labelMedium)
                Text(w.address, style = MaterialTheme.typography.bodySmall,
                    fontFamily = FontFamily.Monospace)
            }

            Spacer(Modifier.height(16.dp))
            OutlinedTextField(
                value = passphrase,
                onValueChange = { passphrase = it; error = null },
                label = { Text("Passphrase") },
                supportingText = {
                    Text(
                        if (longEnough) "Good" else "At least 8 characters",
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
                        val w = state.wallet
                        val persona = state.persona
                        if (w == null || persona == null) {
                            error = "Setup is incomplete"
                            return@Button
                        }
                        error = try {
                            val bytes = exportBackup(
                                BackupInput(
                                    w.spendKeyHex,
                                    w.restoreHeight,
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
                            written = f
                            null
                        } catch (t: Throwable) {
                            t.message ?: t::class.simpleName
                        }
                    },
                    enabled = longEnough,
                ) { Text("Create backup") }
            } else {
                Text(
                    "Backup created — ${written!!.length()} bytes.",
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    "It is on this phone, which is not a backup yet. Send it " +
                        "somewhere else: a password manager, a drive, another device.",
                    style = MaterialTheme.typography.bodySmall,
                )
                Spacer(Modifier.height(8.dp))
                Row {
                    Button(onClick = { shareBackup(context, written!!) }) { Text("Send it somewhere") }
                    Spacer(Modifier.width(8.dp))
                    TextButton(onClick = onDone) { Text("Done") }
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
    context.startActivity(Intent.createChooser(send, "Save your DUCAT backup"))
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
            Text("What should people call you?", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(8.dp))
            Text(
                "This goes on the cards you hand out. Whoever adds you can rename " +
                    "you on their side, and that name is the one they see — so this " +
                    "is a suggestion, not an identity.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(14.dp))
            OutlinedTextField(
                value = name,
                onValueChange = { if (it.length <= 32) name = it },
                label = { Text("Name (optional)") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )

            Spacer(Modifier.height(22.dp))
            Text("Can contacts pay you directly?", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(8.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Switch(checked = publish, onCheckedChange = { publish = it })
                Spacer(Modifier.width(12.dp))
                Text(
                    if (publish) "Yes — easier to be paid"
                    else "No — they ask, you approve",
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
            Spacer(Modifier.height(8.dp))
            Text(
                if (publish) {
                    "Your address goes to each contact and gets reused. Anyone " +
                        "reading the chain can tell the same person was paid each " +
                        "time — including people who only ever paid you once."
                } else {
                    "Each payment uses a fresh address, so nothing on the chain ties " +
                        "your payments together. Someone paying you waits for you to " +
                        "send a request first."
                },
                style = MaterialTheme.typography.bodySmall,
                color = if (publish) MaterialTheme.ducat.changePending
                        else MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            Text(
                "You can change both later.",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.outline,
            )

            Spacer(Modifier.height(18.dp))
            Button(
                onClick = { onNext(name.trim(), publish) },
                modifier = Modifier.fillMaxWidth(),
            ) { Text("Continue") }
        }
    }
}
