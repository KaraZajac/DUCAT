package org.ducatproject.ducat.ui

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.Spring
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
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.scale
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
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

/** A bill, full screen: the decision gets the whole display. */
@Composable
fun BillScreen(
    m: StoredMessage,
    contact: Contact,
    onPay: () -> Unit,
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
                    IconButton(onClick = onClose) { Icon(Icons.Filled.Close, "Close") }
                }
                Spacer(Modifier.height(12.dp))
                Avatar(contact.displayName(), contact.avatar, size = 72)
                Spacer(Modifier.height(10.dp))
                Text(contact.displayName(), style = MaterialTheme.typography.titleLarge)
                Text(
                    "asks you for",
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
                if (m.body.isNotBlank() && m.body != "Payment request") {
                    Spacer(Modifier.height(6.dp))
                    Text("“${m.body}”", style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }

                if (m.items.isNotEmpty()) {
                    Spacer(Modifier.height(16.dp))
                    Card(Modifier.fillMaxWidth().padding(horizontal = 24.dp)) {
                        Column(Modifier.padding(14.dp)) {
                            m.items.forEach { i ->
                                Row(Modifier.fillMaxWidth().padding(vertical = 3.dp)) {
                                    Text(i.description,
                                        style = MaterialTheme.typography.bodyMedium,
                                        modifier = Modifier.weight(1f))
                                    Text(formatXmr(i.amountPxmr),
                                        style = MaterialTheme.typography.bodySmall,
                                        fontFamily = FontFamily.Monospace)
                                }
                            }
                            m.taxPxmr?.let {
                                HorizontalDivider(
                                    Modifier.padding(vertical = 4.dp),
                                    color = MaterialTheme.colorScheme.outlineVariant,
                                )
                                Row(Modifier.fillMaxWidth()) {
                                    Text("tax", style = MaterialTheme.typography.bodyMedium,
                                        modifier = Modifier.weight(1f))
                                    Text(formatXmr(it),
                                        style = MaterialTheme.typography.bodySmall,
                                        fontFamily = FontFamily.Monospace)
                                }
                            }
                        }
                    }
                }

                Spacer(Modifier.weight(1f))
                Column(Modifier.fillMaxWidth().padding(24.dp)) {
                    Button(
                        onClick = onPay,
                        enabled = m.payto != null,
                        modifier = Modifier.fillMaxWidth().height(54.dp),
                    ) { Text("Accept & pay", style = MaterialTheme.typography.titleMedium) }
                    if (m.payto == null) {
                        Text(
                            "No address in this request — ask them where to send it.",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.outline,
                        )
                    }
                    Spacer(Modifier.height(8.dp))
                    OutlinedButton(
                        onClick = onDecline,
                        modifier = Modifier.fillMaxWidth().height(48.dp),
                    ) { Text("Decline", color = MaterialTheme.colorScheme.error) }
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "Accept opens the confirm screen — nothing moves until " +
                            "you approve it there.",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.outline,
                    )
                }
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
                    "Paid" + (toName?.let { " · $it" } ?: "") + " ✓",
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
                    IconButton(onClick = onClose) { Icon(Icons.Filled.Close, "Close") }
                }
                Spacer(Modifier.height(12.dp))
                Avatar(contact.displayName(), contact.avatar, size = 72)
                Spacer(Modifier.height(10.dp))
                Text(contact.displayName(), style = MaterialTheme.typography.titleLarge)
                Text(
                    "offers to drive you for",
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
                    Text(
                        "about ${(secs / 60).coerceAtLeast(1)} min away",
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
                    Text("plate $plate", style = MaterialTheme.typography.bodyMedium,
                        fontFamily = FontFamily.Monospace,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                if (m.body.isNotBlank()) {
                    Spacer(Modifier.height(6.dp))
                    Text("“${m.body}”", style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                Spacer(Modifier.weight(1f))
                Column(Modifier.fillMaxWidth().padding(24.dp)) {
                    Button(
                        onClick = onAccept,
                        modifier = Modifier.fillMaxWidth().height(54.dp),
                    ) { Text("Accept", style = MaterialTheme.typography.titleMedium) }
                    Spacer(Modifier.height(8.dp))
                    OutlinedButton(
                        onClick = onDecline,
                        modifier = Modifier.fillMaxWidth().height(48.dp),
                    ) { Text("Decline", color = MaterialTheme.colorScheme.error) }
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "Accepting agrees the fare — you pay at the end of the " +
                            "ride, through the usual confirm screen.",
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
                    IconButton(onClick = onDismiss) { Icon(Icons.Filled.Close, "Close") }
                }
                Spacer(Modifier.weight(0.6f))
                Text("🚕", style = MaterialTheme.typography.displaySmall)
                Spacer(Modifier.height(8.dp))
                Text("Ride confirmed", style = MaterialTheme.typography.titleLarge)
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
                Text(
                    "They're expecting you — anything else goes in the chat.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.weight(1f))
                Button(
                    onClick = onOpenChat,
                    modifier = Modifier.fillMaxWidth().height(52.dp),
                ) { Text("Open the chat") }
            }
        }
    }
}

/** A stranger becomes your ride: the card you scan the curb with. */
@Composable
fun DriverFound(contact: Contact, onOpenChat: () -> Unit, onDismiss: () -> Unit) {
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
                    IconButton(onClick = onDismiss) { Icon(Icons.Filled.Close, "Close") }
                }
                Spacer(Modifier.weight(0.6f))
                Text("🚕", style = MaterialTheme.typography.displaySmall)
                Spacer(Modifier.height(8.dp))
                Text("Your ride is coming", style = MaterialTheme.typography.titleLarge)
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
                Text(
                    "ETA and anything else — in the chat.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.weight(1f))
                Button(
                    onClick = onOpenChat,
                    modifier = Modifier.fillMaxWidth().height(52.dp),
                ) { Text("Open the chat") }
            }
        }
    }
}
