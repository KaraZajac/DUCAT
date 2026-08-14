package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.BillItem
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.RateStore
import org.ducatproject.ducat.TabStore
import org.ducatproject.ducat.formatXmr
import java.math.BigDecimal

private const val TAG = "Taxi"

/**
 * The meter (§15.11): negotiate at the start, settle at the end.
 *
 * The one requirement the spec adds is about consent, not formatting: the rate
 * goes into the thread as a message **when the meter starts** — a fare the
 * rider agreed to before the wheels moved — and the bill at the end shows the
 * arithmetic in its line items, which §16.13's sum rule makes checkable. A
 * fiat rate is snapshotted to piconero at start and used unchanged: exchange
 * movement during the ride is the driver's exposure, who chose the quoting
 * currency.
 *
 * Settlement rides the same rails as a bar tab — the ride ends as a settled
 * tab, and the poller sends the receipt when the fare lands, which may well be
 * after the rider has walked away. That case is the §16.12 machinery's whole
 * point.
 */

/** The active ride, in plain prefs: at most one, because the car has one seat row. */
private class RideStore(context: android.content.Context) {
    private val prefs = context.getSharedPreferences("ducat_contacts", android.content.Context.MODE_PRIVATE)
    fun personaHex(): String? = prefs.getString("ride_persona", null)
    fun startedAt(): Long = prefs.getLong("ride_started", 0L)
    fun basePxmr(): Long = prefs.getLong("ride_base", 0L)
    fun perMinPxmr(): Long = prefs.getLong("ride_per_min", 0L)
    fun start(persona: String, base: Long, perMin: Long) {
        prefs.edit().putString("ride_persona", persona)
            .putLong("ride_started", System.currentTimeMillis())
            .putLong("ride_base", base).putLong("ride_per_min", perMin).apply()
        org.ducatproject.ducat.ContactStore.bump()
    }
    fun clear() {
        prefs.edit().remove("ride_persona").remove("ride_started")
            .remove("ride_base").remove("ride_per_min").apply()
        org.ducatproject.ducat.ContactStore.bump()
    }
}

@Composable
fun TaxiScreen() {
    val context = LocalContext.current
    val version by ContactStore.changes.collectAsState()
    val rides = remember { RideStore(context) }
    val active = remember(version) { rides.personaHex() }

    if (active != null) {
        MeterScreen(rides, active)
    } else {
        NewRideScreen(rides)
    }
}

/** Rate setup and the pickup code. The rate is typed once and remembered. */
@Composable
private fun NewRideScreen(rides: RideStore) {
    val context = LocalContext.current
    val prefs = remember {
        context.getSharedPreferences("ducat_contacts", android.content.Context.MODE_PRIVATE)
    }
    // Empty fields inherit the fare card's positioned defaults (§15.12's
    // pricing: inside the platform margin) rather than making every driver
    // invent a rate at the curb.
    var base by remember { mutableStateOf(
        prefs.getString("taxi_base_text", null)
            ?: "%.2f".format(org.ducatproject.ducat.Fare.base(context))
    ) }
    var perMin by remember { mutableStateOf(
        prefs.getString("taxi_permin_text", null)
            ?: "%.2f".format(org.ducatproject.ducat.Fare.perMin(context))
    ) }
    var fiat by remember { mutableStateOf(Amounts.preferFiat(context)) }
    val rate = remember { RateStore(context).cached()?.first }
    val cur = remember { Amounts.currency(context) }
    var cardUri by remember { mutableStateOf<String?>(null) }
    var cardInbox by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }

    fun toPxmr(text: String): Long? {
        val v = text.toBigDecimalOrNull() ?: return null
        val xmr = if (fiat) {
            if (rate == null || rate <= 0) return null
            v.divide(BigDecimal(rate), 12, java.math.RoundingMode.DOWN)
        } else v
        return runCatching { xmr.movePointRight(12).toLong() }.getOrNull()?.takeIf { it >= 0 }
    }

    val basePxmr = toPxmr(base.ifBlank { "0" })
    val perMinPxmr = toPxmr(perMin)
    val ready = basePxmr != null && perMinPxmr != null && perMinPxmr > 0

    DisposableEffect(cardUri) {
        org.ducatproject.ducat.nfc.Tap.offered = cardUri
        onDispose { org.ducatproject.ducat.nfc.Tap.offered = null }
    }

    LaunchedEffect(Unit) {
        val r = withContext(Dispatchers.IO) {
            runCatching {
                Mailbox.issueCard(
                    context, MyProfile(context).name(), 60uL * 60uL * 12uL, purpose = "sale",
                )
            }
        }
        r.onSuccess { cardUri = it.uri; cardInbox = it.inboxKey }
            .onFailure { error = it.message ?: "could not publish a code" }
    }

    // Scan → terms into the thread → meter starts. The rider has the rate in
    // writing before the wheels move, which is the point.
    LaunchedEffect(cardInbox, ready, basePxmr, perMinPxmr) {
        val inbox = cardInbox
        if (inbox == null || !ready) return@LaunchedEffect
        while (true) {
            delay(2_000)
            val fresh = withContext(Dispatchers.IO) {
                runCatching {
                    Mailbox.collectClaims(context)
                    ContactStore(context).claimantOf(inbox)?.let { hex ->
                        ContactStore(context).all().firstOrNull { it.personaHex == hex }
                    }
                }.getOrNull()
            } ?: continue
            withContext(Dispatchers.IO) {
                runCatching {
                    Mailbox.send(
                        context, fresh,
                        "Meter started — base ${formatXmr(basePxmr!!)} XMR, " +
                            "${formatXmr(perMinPxmr!!)} XMR per minute. " +
                            "The bill at the end shows the minutes.",
                        PersonaStore(context).personaHex(),
                    )
                }.onSuccess {
                    prefs.edit().putString("taxi_base_text", base)
                        .putString("taxi_permin_text", perMin).apply()
                    rides.start(fresh.personaHex, basePxmr, perMinPxmr)
                    DucatLog.i(TAG, "ride started with ${fresh.displayName()}")
                }.onFailure { error = "They connected but the terms did not send: ${it.message}" }
            }
            break
        }
    }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("Set the fare", style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(10.dp))
        Row {
            OutlinedTextField(
                value = base,
                onValueChange = { base = it.filter { c -> c.isDigit() || c == '.' } },
                label = { Text("Base (${if (fiat) cur else "XMR"})") },
                singleLine = true,
                modifier = Modifier.weight(1f),
            )
            Spacer(Modifier.width(8.dp))
            OutlinedTextField(
                value = perMin,
                onValueChange = { perMin = it.filter { c -> c.isDigit() || c == '.' } },
                label = { Text("Per min (${if (fiat) cur else "XMR"})") },
                singleLine = true,
                modifier = Modifier.weight(1f),
            )
        }
        if (rate != null) {
            TextButton(onClick = { fiat = !fiat; base = ""; perMin = "" }) {
                Text("Price in ${if (fiat) "XMR" else cur} instead")
            }
        }
        // Snapshotted, and said here because it is a real term of the deal.
        Text(
            "A rate typed in $cur is locked to XMR when the ride starts — the " +
                "figure the rider agrees to is the figure they are billed.",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.outline,
        )

        Spacer(Modifier.height(20.dp))
        when {
            !ready -> Text(
                "Set a per-minute rate to open the pickup code.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            cardUri != null -> {
                Text("Pickup", style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(8.dp))
                QrBlock(cardUri!!)
                Spacer(Modifier.height(8.dp))
                Text(
                    "The rider scans or taps. The rate lands in the conversation " +
                        "and the meter starts by itself.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
            }
            error != null -> Text(error!!, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall)
            else -> CircularProgressIndicator()
        }
    }
}

/** The running meter: elapsed, fare so far, and the one button that ends it. */
@Composable
private fun MeterScreen(rides: RideStore, personaHex: String) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var now by remember { mutableStateOf(System.currentTimeMillis()) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val contact = remember(personaHex) {
        ContactStore(context).all().firstOrNull { it.personaHex == personaHex }
    }

    // A ticking clock, not a stored counter: the fare is *derived* from the
    // start time, so a killed process resumes at the right figure.
    LaunchedEffect(Unit) { while (true) { delay(1_000); now = System.currentTimeMillis() } }

    val secs = ((now - rides.startedAt()) / 1000).coerceAtLeast(0)
    val minutes = secs / 60
    val metered = rides.perMinPxmr() * secs / 60
    val fare = rides.basePxmr() + metered

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Spacer(Modifier.height(8.dp))
        Text(
            "%d:%02d".format(secs / 60, secs % 60),
            style = MaterialTheme.typography.headlineMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(6.dp))
        val shown = Amounts.show(context, fare)
        Text(shown.primary, style = MaterialTheme.typography.displayLarge)
        shown.secondary?.let {
            Text(it, style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Spacer(Modifier.height(4.dp))
        Text(
            "base ${formatXmr(rides.basePxmr())} + " +
                "${formatXmr(rides.perMinPxmr())}/min · " +
                (contact?.displayName() ?: "rider"),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        Spacer(Modifier.height(28.dp))
        Button(
            onClick = {
                busy = true; error = null
                // The figures are frozen at the tap, then billed: base as one
                // line, the metered time as another whose description carries
                // the arithmetic — §15.11's checkable meter.
                val endSecs = ((System.currentTimeMillis() - rides.startedAt()) / 1000)
                    .coerceAtLeast(0)
                val meteredEnd = rides.perMinPxmr() * endSecs / 60
                val lines = buildList {
                    if (rides.basePxmr() > 0) add(BillItem("Base fare", rides.basePxmr()))
                    add(BillItem(
                        "%d min %02d s × %s XMR/min".format(
                            endSecs / 60, endSecs % 60, formatXmr(rides.perMinPxmr()),
                        ),
                        meteredEnd,
                    ))
                }
                scope.launch {
                    val r = withContext(Dispatchers.IO) {
                        runCatching {
                            val store = TabStore(context)
                            val tab = store.open(personaHex, "taxi")
                            store.update(tab.copy(lines = lines))
                            store.settle(store.get(tab.id)!!)
                        }
                    }
                    busy = false
                    r.onSuccess { rides.clear() }
                        .onFailure { error = it.message ?: "could not send the fare" }
                }
            },
            enabled = !busy,
            modifier = Modifier.fillMaxWidth().height(56.dp),
        ) {
            if (busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
            else Text("End ride — bill ${Amounts.show(context, fare).primary}")
        }
        Text(
            "The bill shows the minutes and the rate. The receipt goes to them " +
                "by itself when the fare lands — even after they have walked away.",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.outline,
            textAlign = androidx.compose.ui.text.style.TextAlign.Center,
            modifier = Modifier.padding(top = 8.dp),
        )

        Spacer(Modifier.height(10.dp))
        TextButton(onClick = { rides.clear() }) {
            Text("Cancel ride — no bill", color = MaterialTheme.colorScheme.error)
        }
        error?.let {
            Text(it, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall)
        }
    }
}
