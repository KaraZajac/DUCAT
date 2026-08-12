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
import uniffi.ducat_mobile.NewWallet
import uniffi.ducat_mobile.createWallet

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
enum class Step { Persona, Wallet, Limits, Backup, Done }

data class Onboarding(
    val step: Step = Step.Persona,
    val wallet: NewWallet? = null,
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
                onAction = { onState(state.copy(step = Step.Wallet)) },
            )

            Step.Wallet -> StepCard(
                title = "Create your wallet",
                body = "Monero keys, generated here and held here. This is the money.",
                action = "Create wallet",
                onAction = {
                    // Real keys from the native core. The height is the chain tip
                    // a node reports; for a *fresh* wallet that is correct, since
                    // it has no earlier outputs to miss — the one case where
                    // "now" is right rather than catastrophic (§4.3.1).
                    val w = createWallet(tipHeight = 0uL, stagenet = true)
                    onState(state.copy(step = Step.Limits, wallet = w))
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
                wallet = state.wallet,
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
        Step.Persona -> 1; Step.Wallet -> 2; Step.Limits -> 3
        Step.Backup -> 4; Step.Done -> 4
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
            if (step == Step.Done) "Done" else "Step $n of 4",
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
private fun BackupStep(wallet: NewWallet?, onDone: () -> Unit) {
    var passphrase by remember { mutableStateOf("") }
    var confirmed by remember { mutableStateOf(false) }
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

            if (wallet != null) {
                Spacer(Modifier.height(12.dp))
                Text("Your address", style = MaterialTheme.typography.labelMedium)
                Text(
                    wallet.address,
                    style = MaterialTheme.typography.bodySmall,
                    fontFamily = FontFamily.Monospace,
                )
            }

            Spacer(Modifier.height(16.dp))
            OutlinedTextField(
                value = passphrase,
                onValueChange = { passphrase = it },
                label = { Text("Passphrase") },
                supportingText = {
                    // §4.3.2 refuses trivially short passphrases at export rather
                    // than producing an artifact whose protection is nominal.
                    Text(
                        if (longEnough) "Good" else "At least 8 characters",
                        color = if (longEnough) MaterialTheme.ducat.settled
                                else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )

            Spacer(Modifier.height(8.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Checkbox(checked = confirmed, onCheckedChange = { confirmed = it })
                Text("I've saved the file somewhere I can find it")
            }

            Spacer(Modifier.height(8.dp))
            Button(onClick = onDone, enabled = longEnough && confirmed) {
                Text("Export backup")
            }
        }
    }
}
