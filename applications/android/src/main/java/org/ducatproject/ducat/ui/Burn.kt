package org.ducatproject.ducat.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.R
import org.ducatproject.ducat.Trust
import org.ducatproject.ducat.WalletStore

/**
 * §9.5's screen: giving up money so that a name costs something.
 *
 * The hardest thing about this screen is that it is a button which destroys
 * money on purpose, and the person pressing it has to understand that before
 * they press it rather than after. So the page says it in the order somebody
 * reads: what a burn is, what it buys, where the money goes, and only then
 * the amount and the button — with the PIN between the button and the send,
 * exactly as a payment has it.
 *
 * What it deliberately does not do is sell the idea. There is no score, no
 * badge and no number climbing: a burn is a cost, and the screen's job is to
 * make the cost legible, not attractive.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun BurnScreen(onClose: () -> Unit) {
    val context = LocalContext.current
    BackHandler(onBack = onClose)
    val scope = rememberCoroutineScope()
    val version by ContactStore.changes.collectAsState()

    val roster = remember { PersonaStore(context) }
    val worn = remember(version) { roster.worn() }
    val wornPersona = remember(version, worn) { roster.all().firstOrNull { it.hex == worn } }
    val stagenet = remember { WalletStore(context).stagenet() }
    val address = remember(version) { Trust.burnAddress(context) }
    // This persona's burns only. A burn belongs to the name it was made
    // under, and showing another compartment's here would tie the two
    // together on the one screen whose whole point is that they are not.
    val mine = remember(version, worn) {
        Trust.burns(context).filter { it.personaHex.equals(worn, ignoreCase = true) }
            .sortedByDescending { it.madeAt }
    }

    var amount by rememberSaveable { mutableStateOf("0.01") }
    var purpose by rememberSaveable { mutableStateOf(Trust.DEFAULT_PURPOSE) }
    var busy by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    var askPin by remember { mutableStateOf(false) }

    val pxmr = remember(amount) { Amounts.parse(amount)?.let { Amounts.toPxmr(it) } }
    // The store's own rule, asked here so the button is dark for the same
    // reasons the burn would be refused — and null when there is nothing to
    // say, which is not the same as a field that is not a number yet.
    val refusal = remember(pxmr, purpose) { pxmr?.let { Trust.refusal(it, purpose) } }
    val ready = pxmr != null && refusal == null

    // The application context, not this screen's: the send outlives a back
    // press, and the record is written inside Trust.burn — so leaving here
    // mid-burn costs the sentence at the bottom of the screen, never the
    // proof.
    val app = context.applicationContext
    val doBurn: () -> Unit = doBurn@{
        val amt = pxmr ?: return@doBurn
        busy = true
        problem = null
        val label = purpose
        scope.launch {
            val done = withContext(Dispatchers.IO) {
                runCatching { Trust.burn(app, worn, amt, label) }
            }
            busy = false
            done.onFailure {
                problem = it.message?.ifBlank { null } ?: app.getString(R.string.burn_failed)
            }
        }
    }

    PinGate(
        open = askPin,
        onDismiss = { askPin = false },
        onPassed = { askPin = false; doBurn() },
        why = R.string.burn_pin_why,
    )

    Dialog(onDismissRequest = onClose, properties = fullScreenDialogProperties()) {
      Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
        Scaffold(
            topBar = {
                TopAppBar(
                    colors = TopAppBarDefaults.topAppBarColors(
                        containerColor = MaterialTheme.colorScheme.background,
                    ),
                    title = { Text(stringResource(R.string.burn_title)) },
                    navigationIcon = {
                        IconButton(onClick = onClose) {
                            Icon(
                                Icons.AutoMirrored.Filled.ArrowBack,
                                contentDescription = stringResource(R.string.burn_back),
                            )
                        }
                    },
                )
            },
        ) { padding ->
            Column(
                Modifier.padding(padding).fillMaxSize()
                    .verticalScroll(rememberScrollState())
                    .padding(16.dp),
            ) {
                // What it is, before anything can be typed.
                Text(
                    stringResource(R.string.burn_what_body),
                    style = MaterialTheme.typography.bodyMedium,
                )
                Spacer(Modifier.height(10.dp))
                Text(
                    stringResource(R.string.burn_what_get),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(20.dp))

                Card(Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(16.dp)) {
                        Text(
                            stringResource(
                                R.string.burn_persona,
                                wornPersona?.let { personaLabel(it) }
                                    ?: stringResource(R.string.personas_primary),
                            ),
                            style = MaterialTheme.typography.labelLarge,
                        )
                        Spacer(Modifier.height(12.dp))
                        OutlinedTextField(
                            value = amount,
                            onValueChange = { amount = moneyText(it); problem = null },
                            label = { Text(stringResource(R.string.burn_amount_label)) },
                            singleLine = true,
                            modifier = Modifier.fillMaxWidth(),
                        )
                        Spacer(Modifier.height(6.dp))
                        Text(
                            stringResource(
                                R.string.burn_floor,
                                Amounts.show(context, Trust.FLOOR_PXMR, stagenet).primary,
                            ),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Spacer(Modifier.height(12.dp))
                        OutlinedTextField(
                            value = purpose,
                            onValueChange = {
                                // Cut at the wire's own limit rather than
                                // refusing after the money is gone: the
                                // envelope will not carry a longer label.
                                // Counted in characters, as the envelope
                                // counts them — sixteen emoji are sixteen.
                                if (it.codePointCount(0, it.length) <= Trust.MAX_PURPOSE_CHARS) {
                                    purpose = it
                                }
                                problem = null
                            },
                            label = { Text(stringResource(R.string.burn_purpose_label)) },
                            supportingText = {
                                Text(stringResource(R.string.burn_purpose_support))
                            },
                            singleLine = true,
                            modifier = Modifier.fillMaxWidth(),
                        )
                        Spacer(Modifier.height(16.dp))
                        Text(
                            stringResource(R.string.burn_irreversible),
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.error,
                        )
                        Spacer(Modifier.height(10.dp))
                        Button(
                            enabled = !busy && ready,
                            onClick = { problem = null; askPin = true },
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            if (busy) {
                                CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                            } else {
                                Text(
                                    stringResource(
                                        R.string.burn_action,
                                        Amounts.show(context, pxmr ?: 0L, stagenet).primary,
                                    ),
                                )
                            }
                        }
                        // Why the button is dark, said where the button is.
                        refusal?.let {
                            Spacer(Modifier.height(8.dp))
                            Text(
                                Trust.words(context, it),
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        problem?.let {
                            Spacer(Modifier.height(8.dp))
                            Text(
                                it,
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.error,
                            )
                        }
                    }
                }

                Spacer(Modifier.height(16.dp))
                Card(Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(16.dp)) {
                        Text(
                            stringResource(R.string.burn_address_title),
                            style = MaterialTheme.typography.titleMedium,
                        )
                        Spacer(Modifier.height(6.dp))
                        Text(
                            stringResource(R.string.burn_address_note),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Spacer(Modifier.height(12.dp))
                        SelectionContainer {
                            Text(
                                address,
                                fontFamily = FontFamily.Monospace,
                                style = MaterialTheme.typography.bodySmall,
                            )
                        }
                        Spacer(Modifier.height(12.dp))
                        OutlinedButton(
                            onClick = {
                                copyText(
                                    context, address,
                                    context.getString(R.string.burn_address_copied),
                                )
                            },
                            modifier = Modifier.fillMaxWidth(),
                        ) { Text(stringResource(R.string.burn_copy_address)) }
                    }
                }

                Spacer(Modifier.height(24.dp))
                Text(
                    stringResource(R.string.burn_yours),
                    style = MaterialTheme.typography.titleMedium,
                )
                Spacer(Modifier.height(8.dp))
                if (mine.isEmpty()) {
                    Text(
                        stringResource(R.string.burn_none),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                for (b in mine) {
                    Card(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
                        Column(Modifier.padding(16.dp)) {
                            Text(
                                Amounts.show(context, b.amountPxmr, stagenet).primary,
                                style = MaterialTheme.typography.titleMedium,
                            )
                            Text(
                                b.purpose,
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                            Spacer(Modifier.height(8.dp))
                            Text(
                                when {
                                    b.proof.isBlank() ->
                                        stringResource(R.string.burn_no_proof)
                                    b.finished ->
                                        stringResource(
                                            R.string.burn_in_block,
                                            Amounts.count(b.height),
                                        )
                                    else -> stringResource(R.string.burn_waiting)
                                },
                                style = MaterialTheme.typography.bodyMedium,
                            )
                            val envelope = b.envelopeHex
                            if (b.finished && envelope != null) {
                                Spacer(Modifier.height(10.dp))
                                // As the link a thread recognises (spec
                                // §9.5 "How a proof travels"), so pasting
                                // it into any chat, on either client, draws
                                // a proof rather than a wall of hex.
                                OutlinedButton(
                                    onClick = {
                                        copyText(
                                            context, "ducat:burn/$envelope",
                                            context.getString(R.string.burn_proof_copied),
                                        )
                                    },
                                    modifier = Modifier.fillMaxWidth(),
                                ) { Text(stringResource(R.string.burn_copy_proof)) }
                            }
                        }
                    }
                }
                Spacer(Modifier.height(24.dp))
            }
        }
      }
    }
}
