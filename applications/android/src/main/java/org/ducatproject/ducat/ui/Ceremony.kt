package org.ducatproject.ducat.ui

import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.animation.core.Spring
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.delay
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.Contact
import org.ducatproject.ducat.R
import org.ducatproject.ducat.StoredMessage
import org.ducatproject.ducat.formatXmr

/**
 * Money moments get ceremony.
 *
 * A bill is not a chat bubble, and a completed payment is not a status line —
 * every pay terminal on Earth knows the full-screen beat matters, because the
 * moment money moves is the moment both people look at the screen. The
 * ceremonies live here: the bill (a decision), the paid splash (a relief),
 * and the ride's three beats — the driver's offer, the rider's yes, the car
 * at the curb. None of them touch the wire; they are presentation over
 * §16.13's machinery, and the confirm screen they lead to remains the only
 * thing between a message and money leaving.
 */

/**
 * A bill, full screen: the decision gets the whole display.
 *
 * [over] is the sentence that says the decision has already been made —
 * withdrawn by its sender, declined here, or paid — and with one the
 * screen offers nothing to press. Found live (2026-09-01): the bar
 * withdrew a bill while the customer had it open; the retraction arrived,
 * the bubble underneath greyed to "Cancelled", and this screen went on
 * offering "Accept & pay" for money nobody was watching for (§15.11),
 * one tap from the confirm screen. The caller works the verdict out from
 * the thread as it stands *now*; this screen only shows it.
 */
@Composable
fun BillScreen(
    m: StoredMessage,
    contact: Contact,
    onPay: () -> Unit,
    onDecline: () -> Unit,
    onClose: () -> Unit,
    over: String? = null,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    Dialog(
        onDismissRequest = onClose,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            Column(
                Modifier.fillMaxSize().verticalScroll(rememberScrollState()),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Row(Modifier.fillMaxWidth().padding(8.dp)) {
                    IconButton(onClick = onClose) { Icon(Icons.Filled.Close, stringResource(R.string.ceremony_close)) }
                }
                Spacer(Modifier.height(12.dp))
                Avatar(contact.displayName(), contact.avatar, size = 72)
                Spacer(Modifier.height(10.dp))
                Text(contact.displayName(), style = MaterialTheme.typography.titleLarge)
                Text(
                    stringResource(R.string.ceremony_asks_you_for),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(8.dp))
                val shown = Amounts.show(context, m.amountPxmr)
                Text(shown.primary, style = MaterialTheme.typography.displayLarge)
                shown.secondary?.let {
                    Text(it, style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                // In whatever language their phone was in — see
                // `everyTranslationOf`.
                val filler = org.ducatproject.ducat.Languages.everyTranslationOf(
                    context, R.string.pay_payment_request,
                )
                if (m.body.isNotBlank() && m.body !in filler) {
                    Spacer(Modifier.height(6.dp))
                    // Fenced: the quotation marks are in the reader's
                    // direction and the words in the writer's — see isolate.
                    Text(stringResource(R.string.ceremony_quoted_body, isolate(m.body)),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }

                if (m.items.isNotEmpty()) {
                    Spacer(Modifier.height(16.dp))
                    Card(Modifier.fillMaxWidth().padding(horizontal = 24.dp)) {
                        Column(Modifier.padding(14.dp)) {
                            // In the unit this wallet reads money in, like the
                            // total above them and like the same list in the
                            // chat. Priced in XMR under a USD headline, the
                            // lines were a receipt nobody could check against
                            // the menu board they had just read: "Flat white
                            // 0.009515" under "USD 8.03".
                            @Composable
                            fun line(label: String, pxmr: Long) {
                                Row(Modifier.fillMaxWidth().padding(vertical = 3.dp)) {
                                    Text(label,
                                        style = MaterialTheme.typography.bodyMedium,
                                        modifier = Modifier.weight(1f))
                                    Text(Amounts.show(context, pxmr).primary,
                                        style = MaterialTheme.typography.bodySmall,
                                        fontFamily = FontFamily.Monospace)
                                }
                            }
                            m.items.forEach { line(it.description, it.amountPxmr) }
                            m.taxPxmr?.let {
                                HorizontalDivider(
                                    Modifier.padding(vertical = 4.dp),
                                    color = MaterialTheme.colorScheme.outlineVariant,
                                )
                                line(stringResource(R.string.ceremony_tax), it)
                            }
                        }
                    }
                }

                Spacer(Modifier.weight(1f))
                Column(Modifier.fillMaxWidth().padding(24.dp)) {
                    if (over != null) {
                        // Nothing to decide any more, so nothing to press:
                        // the verdict where the buttons were, and Close in
                        // the corner as the only way out.
                        Text(
                            over,
                            style = MaterialTheme.typography.bodyLarge,
                            textAlign = TextAlign.Center,
                            modifier = Modifier.fillMaxWidth(),
                        )
                    } else {
                        Button(
                            onClick = onPay,
                            enabled = m.payto != null,
                            modifier = Modifier.fillMaxWidth().height(54.dp),
                        ) { Text(stringResource(R.string.ceremony_accept_and_pay),
                            style = MaterialTheme.typography.titleMedium) }
                        if (m.payto == null) {
                            Text(
                                stringResource(R.string.ceremony_no_address),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.outline,
                            )
                        }
                        Spacer(Modifier.height(8.dp))
                        OutlinedButton(
                            onClick = onDecline,
                            modifier = Modifier.fillMaxWidth().height(48.dp),
                        ) { Text(stringResource(R.string.ceremony_decline),
                            color = MaterialTheme.colorScheme.error) }
                        Spacer(Modifier.height(8.dp))
                        Text(
                            stringResource(R.string.ceremony_accept_opens_confirm),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.outline,
                        )
                    }
                }
            }
        }
    }
}

/**
 * The minute the money is in flight.
 *
 * Building a Monero transaction is not quick: the wallet pulls decoys for
 * every input it spends — the better part of ten megabytes — then signs and
 * broadcasts, and on a phone that is the better part of a minute. All of it
 * used to happen behind a sixteen-density-pixel spinner inside a button, on
 * the screen where somebody has just agreed to send money they cannot get
 * back. The comment beside it said "the few seconds this covers", and it is
 * not a few seconds.
 *
 * So: the same object [PaidSplash] resolves into, held in its unresolved
 * state — the cat, with a circle going round it, because that is the thing
 * that is happening.
 *
 * **No progress bar, deliberately.** The phases are real — tip, decoys, fee,
 * sign, publish — but they live inside one FFI call and this side cannot see
 * them, so any bar drawn here would be a picture of a guess. A screen that
 * invents its own certainty about money is the exact habit the rest of this
 * app spends its time removing. It says what is true instead: roughly how
 * long, why, and that leaving does not stop it.
 */
@Composable
fun SendingSplash(amountPxmr: Long, toName: String?) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val spin = rememberInfiniteTransition(label = "sending")
    // Slow. A fast orbit reads as agitation, and the honest feeling of this
    // minute is patience.
    val angle by spin.animateFloat(
        initialValue = 0f,
        targetValue = 360f,
        animationSpec = infiniteRepeatable(
            animation = tween(3400, easing = LinearEasing),
        ),
        label = "orbit",
    )
    // The cat breathes. One moving thing would look mechanical; two moving at
    // unrelated rates look alive, and nothing here is trying to say anything
    // by it.
    val breath by spin.animateFloat(
        initialValue = 0.97f,
        targetValue = 1.03f,
        animationSpec = infiniteRepeatable(
            animation = tween(2100, easing = FastOutSlowInEasing),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "breath",
    )
    Dialog(
        // Not dismissible: the transaction is already being built, and a
        // screen that can be waved away suggests the thing behind it can be.
        onDismissRequest = {},
        properties = DialogProperties(
            usePlatformDefaultWidth = false,
            dismissOnBackPress = false,
            dismissOnClickOutside = false,
        ),
    ) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surface) {
            Column(
                Modifier.fillMaxSize().padding(horizontal = 32.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
            ) {
                val track = MaterialTheme.colorScheme.outlineVariant
                val sweep = MaterialTheme.colorScheme.primary
                Box(Modifier.size(220.dp), contentAlignment = Alignment.Center) {
                    // A circle going round, drawn as a comet rather than a
                    // Material spinner: the head is bright and the tail fades
                    // out behind it, so a still frame already says which way it
                    // is travelling and roughly how fast.
                    Canvas(Modifier.size(180.dp)) {
                        drawArc(
                            color = track,
                            startAngle = 0f,
                            sweepAngle = 360f,
                            useCenter = false,
                            style = Stroke(width = 1.dp.toPx()),
                        )
                        // Compose measures arcs from three o'clock; the head
                        // rides at twelve. The -90 is that quarter turn.
                        val head = angle - 90f
                        val steps = 24
                        val w = 4.dp.toPx()
                        repeat(steps) { k ->
                            val f = k.toFloat() / steps
                            drawArc(
                                color = sweep.copy(alpha = 0.85f * f * f),
                                startAngle = head - 96f + 96f * f,
                                sweepAngle = 96f / steps + 0.8f,
                                useCenter = false,
                                style = Stroke(width = w, cap = StrokeCap.Round),
                            )
                        }
                    }
                    Surface(
                        shape = CircleShape,
                        color = MaterialTheme.colorScheme.secondaryContainer,
                        tonalElevation = 3.dp,
                        modifier = Modifier.size(140.dp),
                    ) {
                        Box(contentAlignment = Alignment.Center) {
                            Image(
                                painterResource(R.drawable.ducat_cat),
                                contentDescription = null,
                                modifier = Modifier.size(104.dp).scale(breath),
                            )
                        }
                    }
                }
                Spacer(Modifier.height(28.dp))
                val shown = Amounts.show(context, amountPxmr)
                Text(shown.primary, style = MaterialTheme.typography.headlineMedium)
                toName?.let {
                    Spacer(Modifier.height(4.dp))
                    Text(
                        stringResource(R.string.pay_sending_to, isolate(it)),
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Spacer(Modifier.height(20.dp))
                Text(
                    stringResource(R.string.pay_sending_why),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                )
            }
        }
    }
}

/** The relief: money moved, the screen says so with its whole self. */
@Composable
fun PaidSplash(amountPxmr: Long, toName: String?, onDone: () -> Unit) {
    val context = androidx.compose.ui.platform.LocalContext.current
    var grown by remember { mutableStateOf(false) }
    val scale by animateFloatAsState(
        targetValue = if (grown) 1f else 0.3f,
        animationSpec = spring(
            dampingRatio = Spring.DampingRatioMediumBouncy,
            stiffness = Spring.StiffnessLow,
        ),
        label = "paid",
    )
    LaunchedEffect(Unit) {
        grown = true
        delay(2200)
        onDone()
    }
    Dialog(
        onDismissRequest = onDone,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Surface(
            Modifier.fillMaxSize().clickable(onClick = onDone),
            color = MaterialTheme.colorScheme.tertiary,
        ) {
            Column(
                Modifier.fillMaxSize(),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
            ) {
                Surface(
                    shape = CircleShape,
                    color = MaterialTheme.colorScheme.background,
                    modifier = Modifier.size(150.dp).scale(scale),
                ) {
                    Box(contentAlignment = Alignment.Center) {
                        Image(
                            painterResource(R.drawable.ducat_cat),
                            contentDescription = null,
                            modifier = Modifier.size(110.dp),
                        )
                    }
                }
                Spacer(Modifier.height(24.dp))
                val shown = Amounts.show(context, amountPxmr)
                Text(
                    shown.primary,
                    style = MaterialTheme.typography.displayMedium,
                    color = MaterialTheme.colorScheme.onTertiary,
                )
                Text(
                    if (toName != null) stringResource(R.string.ceremony_paid_to, isolate(toName))
                    else stringResource(R.string.ceremony_paid),
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onTertiary,
                )
            }
        }
    }
}

/**
 * The driver's offer (kind 6), full screen: a fare is a decision, like a bill.
 *
 * The fare is the protocol field, so the number shown *is* the number the
 * accept will echo — nothing here re-derives or rounds it. Accepting sends a
 * kind-7 that names this offer; the actual payment still happens at the end
 * of the ride through the ordinary bill-and-confirm path, which is why the
 * buttons say yes to a fare and never to money moving.
 */
@Composable
fun RideOfferScreen(
    m: StoredMessage,
    contact: Contact,
    onAccept: () -> Unit,
    onDecline: () -> Unit,
    onClose: () -> Unit,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    Dialog(
        onDismissRequest = onClose,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            Column(
                Modifier.fillMaxSize().verticalScroll(rememberScrollState()),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Row(Modifier.fillMaxWidth().padding(8.dp)) {
                    IconButton(onClick = onClose) { Icon(Icons.Filled.Close, stringResource(R.string.ceremony_close)) }
                }
                Spacer(Modifier.height(12.dp))
                Avatar(contact.displayName(), contact.avatar, size = 72)
                Spacer(Modifier.height(10.dp))
                Text(contact.displayName(), style = MaterialTheme.typography.titleLarge)
                Text(
                    stringResource(R.string.ceremony_offers_to_drive),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(8.dp))
                val shown = Amounts.show(context, m.amountPxmr)
                Text(shown.primary, style = MaterialTheme.typography.displayLarge)
                shown.secondary?.let {
                    Text(it, style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                m.etaSecs?.let { secs ->
                    Spacer(Modifier.height(6.dp))
                    val mins = (secs / 60).coerceAtLeast(1).toInt()
                    Text(
                        pluralStringResource(R.plurals.ceremony_minutes_away, mins, mins),
                        style = MaterialTheme.typography.titleMedium,
                    )
                }
                val car = listOfNotNull(contact.carColor, contact.carModel)
                    .joinToString(" ").ifBlank { null }
                car?.let {
                    Spacer(Modifier.height(4.dp))
                    Text(it, style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                contact.plate?.let { plate ->
                    Text(stringResource(R.string.ceremony_plate, isolate(plate)),
                        style = MaterialTheme.typography.bodyMedium,
                        fontFamily = FontFamily.Monospace,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                if (m.body.isNotBlank()) {
                    Spacer(Modifier.height(6.dp))
                    Text(stringResource(R.string.ceremony_quoted_body, isolate(m.body)),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                // What accepting actually costs, before accepting — the
                // fare is not the whole number, and finding that out on the
                // next screen is how a good deal starts feeling like a trick.
                val stake = org.ducatproject.ducat.Stakes.stakeFor(
                    org.ducatproject.ducat.Stakes.Deal.Ride, m.amountPxmr,
                )
                if (stake > 0) {
                    Spacer(Modifier.height(10.dp))
                    Text(
                        modifier = Modifier.padding(horizontal = 24.dp),
                        text = stringResource(
                            R.string.ceremony_stake_note,
                            Amounts.show(context, stake).primary,
                            Amounts.show(
                                context,
                                org.ducatproject.ducat.Ceremony.rideFundAmount(m.amountPxmr),
                            ).primary,
                        ),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Spacer(Modifier.weight(1f))
                Column(Modifier.fillMaxWidth().padding(24.dp)) {
                    Button(
                        onClick = onAccept,
                        modifier = Modifier.fillMaxWidth().height(54.dp),
                    ) { Text(stringResource(R.string.ceremony_accept),
                        style = MaterialTheme.typography.titleMedium) }
                    Spacer(Modifier.height(8.dp))
                    OutlinedButton(
                        onClick = onDecline,
                        modifier = Modifier.fillMaxWidth().height(48.dp),
                    ) { Text(stringResource(R.string.ceremony_decline),
                        color = MaterialTheme.colorScheme.error) }
                    Spacer(Modifier.height(8.dp))
                    // **Which escrow this actually builds.**
                    //
                    // "only the two of you can open" is true of the 2-of-2
                    // rung and false of the one above it, and this device
                    // chooses between them a second later, in Hail's accept
                    // handler, from the same ArbiterStore read below. So the
                    // sentence at the moment of consent promised a two-key
                    // escrow to everyone who had ever named an arbiter — and
                    // named nobody, on the one screen where the third
                    // keyholder's identity is the whole question.
                    val arbiterName = remember {
                        org.ducatproject.ducat.ArbiterStore(context).hex()
                            ?.takeIf { it != contact.personaHex }
                            ?.let { h ->
                                org.ducatproject.ducat.ContactStore(context).all()
                                    .firstOrNull { it.personaHex == h }
                            }?.displayName()
                    }
                    Text(
                        if (arbiterName != null) {
                            stringResource(
                                // A fare too small to carry a stake locks the
                                // fare alone (Stakes.stakeFor returns zero
                                // below the floor), and the sentence at the
                                // moment of consent has to say what the escrow
                                // will actually hold.
                                if (stake > 0) R.string.ceremony_accepting_agrees_fare_arbiter
                                else R.string.ceremony_accepting_agrees_fare_arbiter_nostake,
                                isolate(arbiterName),
                            )
                        } else {
                            stringResource(
                                if (stake > 0) R.string.ceremony_accepting_agrees_fare
                                else R.string.ceremony_accepting_agrees_fare_nostake,
                            )
                        },
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.outline,
                    )
                }
            }
        }
    }
}

/**
 * The rider said yes: the driver's half of the confirmation moment.
 *
 * The fare shown is the one the accept echoed, so both screens are looking at
 * the same number when the car pulls out.
 */
@Composable
fun RideConfirmed(
    contact: Contact,
    farePxmr: Long,
    onOpenChat: () -> Unit,
    onDismiss: () -> Unit,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            Column(
                Modifier.fillMaxSize().padding(24.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Row(Modifier.fillMaxWidth()) {
                    IconButton(onClick = onDismiss) { Icon(Icons.Filled.Close, stringResource(R.string.ceremony_close)) }
                }
                Spacer(Modifier.weight(0.6f))
                Text("🚕", style = MaterialTheme.typography.displaySmall)
                Spacer(Modifier.height(8.dp))
                Text(stringResource(R.string.ceremony_ride_confirmed),
                    style = MaterialTheme.typography.titleLarge)
                Spacer(Modifier.height(8.dp))
                val shown = Amounts.show(context, farePxmr)
                Text(shown.primary, style = MaterialTheme.typography.displayMedium)
                shown.secondary?.let {
                    Text(it, style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                Spacer(Modifier.height(20.dp))
                Avatar(contact.displayName(), contact.avatar, size = 88)
                Spacer(Modifier.height(10.dp))
                Text(contact.displayName(), style = MaterialTheme.typography.headlineSmall)
                Spacer(Modifier.height(12.dp))
                // "Post your stake" sends the driver to the chat looking for
                // a button that is not there when the fare is under the
                // stake floor: nothing is asked of them, and the rider pays
                // straight away.
                val stake = org.ducatproject.ducat.Ceremony.rideStakeAmount(farePxmr)
                Text(
                    stringResource(
                        if (stake > 0) R.string.ceremony_expecting_you
                        else R.string.ceremony_expecting_you_nostake,
                    ),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.weight(1f))
                Button(
                    onClick = onOpenChat,
                    modifier = Modifier.fillMaxWidth().height(52.dp),
                ) { Text(stringResource(R.string.ceremony_open_chat)) }
            }
        }
    }
}

/** A stranger becomes your ride: the card you scan the curb with. */
@Composable
fun DriverFound(
    contact: Contact,
    /** The accepted fare — what decides whether there is a stake to wait for. */
    farePxmr: Long,
    onOpenChat: () -> Unit,
    onDismiss: () -> Unit,
) {
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            Column(
                Modifier.fillMaxSize().padding(24.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Row(Modifier.fillMaxWidth()) {
                    IconButton(onClick = onDismiss) { Icon(Icons.Filled.Close, stringResource(R.string.ceremony_close)) }
                }
                Spacer(Modifier.weight(0.6f))
                Text("🚕", style = MaterialTheme.typography.displaySmall)
                Spacer(Modifier.height(8.dp))
                Text(stringResource(R.string.ceremony_ride_coming),
                    style = MaterialTheme.typography.titleLarge)
                Spacer(Modifier.height(20.dp))
                Avatar(contact.displayName(), contact.avatar, size = 88)
                Spacer(Modifier.height(10.dp))
                Text(contact.displayName(), style = MaterialTheme.typography.headlineSmall)
                val car = listOfNotNull(contact.carColor, contact.carModel)
                    .joinToString(" ").ifBlank { null }
                car?.let {
                    Spacer(Modifier.height(4.dp))
                    Text(it, style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                contact.plate?.let { plate ->
                    Spacer(Modifier.height(12.dp))
                    // Drawn like the thing itself: the plate on the screen is
                    // what gets checked against the plate on the bumper, the
                    // one verification only the rider at the curb can run.
                    Box(
                        Modifier
                            .border(3.dp, MaterialTheme.colorScheme.onSurface,
                                RoundedCornerShape(8.dp))
                            .background(MaterialTheme.colorScheme.surfaceVariant,
                                RoundedCornerShape(8.dp))
                            .padding(horizontal = 24.dp, vertical = 10.dp),
                    ) {
                        Text(
                            plate,
                            style = MaterialTheme.typography.headlineMedium,
                            fontFamily = FontFamily.Monospace,
                            fontWeight = FontWeight.Bold,
                        )
                    }
                }
                Spacer(Modifier.height(12.dp))
                // Same rule as RideConfirmed's line: "once their stake is in"
                // names a wait that never ends on a fare too small for one.
                val stake = org.ducatproject.ducat.Ceremony.rideStakeAmount(farePxmr)
                Text(
                    stringResource(
                        if (stake > 0) R.string.ceremony_eta_in_chat
                        else R.string.ceremony_eta_in_chat_nostake,
                    ),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.weight(1f))
                Button(
                    onClick = onOpenChat,
                    modifier = Modifier.fillMaxWidth().height(52.dp),
                ) { Text(stringResource(R.string.ceremony_open_chat)) }
            }
        }
    }
}


/**
 * An escrow moment that asks for money or a signature, given the room it
 * deserves.
 *
 * These lived as three lines and a button in the banner above a conversation —
 * the same strip that carries "building a shared deposit" and other passive
 * status. But posting a stake, securing a fare and signing a split are the
 * points where a person commits money they cannot unsend, and they were the
 * smallest things on the screen. This is the shape the ride screens already
 * use, which is the shape someone recognises as *a decision*: their face, the
 * number large enough to read across a car, the sentence about what happens to
 * it, and nothing else competing.
 *
 * The banner stays. Closing this returns to it, and tapping it opens this
 * again — dismissing a prompt should not hide the thing it was prompting for.
 */
@Composable
fun EscrowStep(
    contact: Contact,
    title: String,
    amountPxmr: Long,
    note: String?,
    action: String,
    onAction: () -> Unit,
    onClose: () -> Unit,
    busy: Boolean = false,
    error: String? = null,
    /** True when the error is the chain saying "not yet". */
    errorWaiting: Boolean = false,
    /** What to add under it — "keeps trying", and how long is left. */
    errorNote: String? = null,
    secondaryLabel: String? = null,
    onSecondary: (() -> Unit)? = null,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    Dialog(
        onDismissRequest = onClose,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            Column(
                Modifier.fillMaxSize().verticalScroll(rememberScrollState()),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Row(Modifier.fillMaxWidth().padding(8.dp)) {
                    IconButton(onClick = onClose) {
                        Icon(Icons.Filled.Close, stringResource(R.string.ceremony_close))
                    }
                }
                Spacer(Modifier.height(8.dp))
                Icon(
                    Icons.Filled.Lock,
                    null,
                    Modifier.size(28.dp),
                    tint = MaterialTheme.colorScheme.primary,
                )
                Spacer(Modifier.height(12.dp))
                Avatar(contact.displayName(), contact.avatar, size = 72)
                Spacer(Modifier.height(10.dp))
                Text(contact.displayName(), style = MaterialTheme.typography.titleLarge)
                Spacer(Modifier.height(6.dp))
                Text(
                    title,
                    style = MaterialTheme.typography.bodyLarge,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(horizontal = 28.dp),
                )
                if (amountPxmr > 0) {
                    Spacer(Modifier.height(12.dp))
                    val shown = Amounts.show(context, amountPxmr)
                    Text(shown.primary, style = MaterialTheme.typography.displaySmall)
                    shown.secondary?.let {
                        Text(
                            it,
                            style = MaterialTheme.typography.titleMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                note?.let {
                    Spacer(Modifier.height(14.dp))
                    Text(
                        it,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.padding(horizontal = 28.dp),
                    )
                }
                error?.let {
                    Spacer(Modifier.height(12.dp))
                    Text(
                        bridgeMessage(it),
                        // Told, not guessed from the wording — the sentence
                        // is a localised plural now and only English has the
                        // word this used to look for.
                        color = if (errorWaiting) {
                            MaterialTheme.colorScheme.onSurfaceVariant
                        } else {
                            MaterialTheme.colorScheme.error
                        },
                        style = MaterialTheme.typography.bodySmall,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.padding(horizontal = 28.dp),
                    )
                    // Said, because otherwise the screen shows the same
                    // sentence for twenty minutes and nothing about it
                    // suggests anyone is still working.
                    errorNote?.let { note ->
                        Text(
                            note,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            style = MaterialTheme.typography.bodySmall,
                            textAlign = TextAlign.Center,
                            modifier = Modifier.padding(horizontal = 28.dp),
                        )
                    }
                }
                Spacer(Modifier.height(28.dp))
                Button(
                    onClick = onAction,
                    enabled = !busy,
                    modifier = Modifier.fillMaxWidth()
                        .padding(horizontal = 24.dp).height(52.dp),
                ) {
                    // Spinner *and* label. A disabled Button paints its
                    // content at 38% alpha, so a lone indicator on the greyed
                    // container came out as a barely-visible dot on a blank
                    // button — the one moment the screen is asking for
                    // patience is the moment it stopped saying for what.
                    if (busy) {
                        CircularProgressIndicator(
                            Modifier.size(18.dp),
                            strokeWidth = 2.dp,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Spacer(Modifier.width(10.dp))
                    }
                    Text(action, style = MaterialTheme.typography.titleMedium)
                }
                if (secondaryLabel != null && onSecondary != null) {
                    Spacer(Modifier.height(8.dp))
                    OutlinedButton(
                        onClick = onSecondary,
                        enabled = !busy,
                        modifier = Modifier.fillMaxWidth()
                            .padding(horizontal = 24.dp).height(48.dp),
                    ) { Text(secondaryLabel) }
                }
                Spacer(Modifier.height(24.dp))
            }
        }
    }
}
