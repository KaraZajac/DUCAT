package org.ducatproject.ducat.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.DucatLog
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.MainActivity
import org.ducatproject.ducat.MyProfile
import org.ducatproject.ducat.formatXmr
import uniffi.ducat_mobile.HailInfo
import uniffi.ducat_mobile.hailDecode
import uniffi.ducat_mobile.hailEncode
import uniffi.ducat_mobile.readContactCard
import uniffi.ducat_mobile.standPost
import uniffi.ducat_mobile.standRead

private const val TAG = "Hail"

/** How long a posted hail stands before the board should ignore it. */
private const val HAIL_TTL_SECS = 15L * 60

/**
 * The rider's half of §15.12, as a card on the personal Home screen.
 *
 * Hailing is a moment, not a job — the modes list is for people running a
 * till or a meter all day, and a rider is neither. So the hail lives where a
 * rider already is: under their balance, folded until wanted. What goes on
 * the board is deliberately nothing: a claim-once card, a coarse destination,
 * an offer, an expiry; the driver who claims it lands in an ordinary
 * conversation, and an unbonded hail is a mutual promise, like flagging a
 * cab — the card says so rather than implying a dispatcher stands behind it.
 */
@Composable
fun HailCard() {
    var expanded by remember { mutableStateOf(false) }
    val context = LocalContext.current
    var cell by remember { mutableStateOf("") }
    var dest by remember { mutableStateOf("") }
    var fareXmr by remember { mutableStateOf("") }
    var posted by remember { mutableStateOf<PostedHail?>(null) }
    var status by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }

    // The wait: the card's inbox answers when a driver claims (§16.9's
    // machinery unchanged). Polling, not a watch — a hail lives minutes.
    LaunchedEffect(posted) {
        val p = posted ?: return@LaunchedEffect
        while (true) {
            delay(3_000)
            val claimant = withContext(Dispatchers.IO) {
                runCatching {
                    Mailbox.collectClaims(context)
                    ContactStore(context).claimantOf(p.inboxKey)
                }.getOrNull()
            }
            if (claimant != null) {
                // Stewardship (§15.12): the notice is spent; clear the slot.
                withContext(Dispatchers.IO) {
                    runCatching { standPost(p.cell, p.subkey, ByteArray(0)) }
                }
                val who = ContactStore(context).all()
                    .firstOrNull { it.personaHex == claimant }?.displayName() ?: "a driver"
                DucatLog.i(TAG, "hail claimed by $who")
                status = "$who took your hail — talk in the chat"
                posted = null
                MainActivity.openChat.value = claimant
                break
            }
        }
    }

    Spacer(Modifier.height(12.dp))
    Card(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
    Column(Modifier.fillMaxWidth().padding(16.dp)) {
        Row(
            Modifier.fillMaxWidth().clickable { if (posted == null) expanded = !expanded },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("🚕", style = MaterialTheme.typography.headlineSmall)
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Text("Hail a ride", style = MaterialTheme.typography.titleMedium)
                Text(
                    if (posted != null) "Hail standing — waiting for a driver"
                    else "Post where you're going to a stand drivers watch.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (posted == null) {
                Icon(
                    if (expanded) Icons.Filled.ExpandLess else Icons.Filled.ExpandMore,
                    null, tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        val p = posted
        if (p == null && expanded) {
            Spacer(Modifier.height(12.dp))
            OutlinedTextField(
                value = cell, onValueChange = { cell = it.take(64) },
                label = { Text("Stand") },
                placeholder = { Text("stand:city-neighborhood") },
                modifier = Modifier.fillMaxWidth(), singleLine = true,
            )
            Text(
                "A stand is a name both sides know — agree on it the way you " +
                    "agree on a corner.",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.outline,
            )
            Spacer(Modifier.height(10.dp))
            OutlinedTextField(
                value = dest, onValueChange = { dest = it.take(64) },
                label = { Text("Where to") },
                placeholder = { Text("airport, terminal B") },
                modifier = Modifier.fillMaxWidth(), singleLine = true,
            )
            Spacer(Modifier.height(10.dp))
            OutlinedTextField(
                value = fareXmr, onValueChange = { fareXmr = it },
                label = { Text("Offer (XMR) — empty asks the driver to quote") },
                modifier = Modifier.fillMaxWidth(), singleLine = true,
            )
            Spacer(Modifier.height(16.dp))
            Button(
                onClick = {
                    busy = true; error = null
                    val theCell = cell.trim()
                    val theDest = dest.trim()
                    val fare = fareXmr.trim().toDoubleOrNull()
                        ?.let { (it * 1e12).toLong().toULong() }
                    kotlinx.coroutines.MainScope().launch(Dispatchers.IO) {
                        runCatching {
                            val card = Mailbox.issueCard(
                                context, MyProfile(context).name(),
                                (HAIL_TTL_SECS * 2).toULong(), purpose = "hail",
                            )
                            val bytes = hailEncode(
                                HailInfo(
                                    card = card.uri,
                                    dest = theDest,
                                    farePxmr = fare,
                                    expiry = (System.currentTimeMillis() / 1000 +
                                        HAIL_TTL_SECS).toULong(),
                                )
                            )
                            val sub = (0..7).random().toUInt()
                            standPost(theCell, sub, bytes)
                            PostedHail(theCell, sub, card.inboxKey)
                        }.onSuccess {
                            posted = it
                            expanded = false
                            status = "Posted. Waiting for a driver…"
                            DucatLog.i(TAG, "hail posted at ${it.cell} subkey ${it.subkey}")
                        }.onFailure {
                            error = it.message ?: "could not post the hail"
                            DucatLog.w(TAG, "post: ${it.message}")
                        }
                        busy = false
                    }
                },
                enabled = !busy && cell.isNotBlank() && dest.isNotBlank(),
                modifier = Modifier.fillMaxWidth().height(52.dp),
            ) {
                if (busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                else Text("Post the hail")
            }
            Spacer(Modifier.height(8.dp))
            Text(
                "A hail is a mutual promise, like flagging a cab — nobody is " +
                    "made to show up. Payment happens at the end, from your " +
                    "confirm screen, never automatically.",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.outline,
            )
        } else if (p != null) {
            Spacer(Modifier.height(10.dp))
            Text("Standing at ${p.cell}",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.height(8.dp))
            LinearProgressIndicator(Modifier.fillMaxWidth())
            Spacer(Modifier.height(10.dp))
            OutlinedButton(
                onClick = {
                    val gone = p
                    posted = null; status = null
                    kotlinx.coroutines.MainScope().launch(Dispatchers.IO) {
                        runCatching { standPost(gone.cell, gone.subkey, ByteArray(0)) }
                    }
                },
                modifier = Modifier.fillMaxWidth(),
            ) { Text("Take it down") }
        }

        status?.let {
            if (posted == null) {
                Spacer(Modifier.height(12.dp))
                Text(it, style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.ducat.settled)
            }
        }
        error?.let {
            Spacer(Modifier.height(12.dp))
            Text(it, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall)
        }
    }
    }
}

private data class PostedHail(val cell: String, val subkey: UInt, val inboxKey: String)

/**
 * The driver's half: watch a stand, take a hail.
 *
 * The claim is the race, and the DHT referees it — a card has one reply slot,
 * so the second driver's claim simply has nowhere to write and this screen
 * reports "someone beat you to it".
 */
@Composable
fun DriveScreen() {
    val context = LocalContext.current
    var cell by remember { mutableStateOf("") }
    var watching by remember { mutableStateOf<String?>(null) }
    var notices by remember { mutableStateOf<List<SeenHail>>(emptyList()) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }

    LaunchedEffect(watching) {
        val c = watching ?: return@LaunchedEffect
        while (true) {
            val fresh = withContext(Dispatchers.IO) {
                runCatching {
                    val now = System.currentTimeMillis() / 1000
                    standRead(c).mapNotNull { n ->
                        runCatching { hailDecode(n.data) }.getOrNull()?.let { h ->
                            if (h.expiry.toLong() > now) {
                                SeenHail(n.subkey, h.card, h.dest,
                                    h.farePxmr?.toLong(), h.expiry.toLong())
                            } else null
                        }
                    }
                }.getOrNull()
            }
            if (fresh != null) notices = fresh
            delay(6_000)
        }
    }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp)) {
        Text("Driving", style = MaterialTheme.typography.headlineSmall)
        Spacer(Modifier.height(4.dp))
        Text(
            "Watch a stand; take a hail. Taking one opens a conversation with " +
                "the rider — quote there, meet there, bill at the end like any " +
                "ride.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(16.dp))

        Row(verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(
                value = cell, onValueChange = { cell = it.take(64) },
                label = { Text("Stand") },
                placeholder = { Text("stand:city-neighborhood") },
                modifier = Modifier.weight(1f), singleLine = true,
                enabled = watching == null,
            )
            Spacer(Modifier.width(8.dp))
            if (watching == null) {
                Button(onClick = { watching = cell.trim() },
                    enabled = cell.isNotBlank()) { Text("Watch") }
            } else {
                OutlinedButton(onClick = {
                    watching = null; notices = emptyList()
                }) { Text("Stop") }
            }
        }

        if (watching != null) {
            Spacer(Modifier.height(16.dp))
            if (notices.isEmpty()) {
                Text(
                    "Nothing on the board. Checking every few seconds…",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            notices.forEach { n ->
                Card(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
                    Column(Modifier.padding(16.dp)) {
                        Text(n.dest, style = MaterialTheme.typography.titleMedium)
                        Text(
                            n.farePxmr?.let { "offers ${formatXmr(it)} XMR" }
                                ?: "asks you to quote",
                            style = MaterialTheme.typography.bodyMedium,
                            fontFamily = FontFamily.Monospace,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        val mins = (n.expiry - System.currentTimeMillis() / 1000) / 60
                        Text("stands for another ${mins} min",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.outline)
                        Spacer(Modifier.height(10.dp))
                        Button(
                            onClick = {
                                busy = true; error = null
                                val theCell = watching!!
                                kotlinx.coroutines.MainScope().launch(Dispatchers.IO) {
                                    runCatching {
                                        val scanned = readContactCard(n.card)
                                        Mailbox.claimCard(context, scanned, null)
                                    }.onSuccess { rider ->
                                        // The notice is spent; clear its slot
                                        // so the next driver is not baited by
                                        // a card that cannot answer (§15.12).
                                        runCatching {
                                            standPost(theCell, n.subkey, ByteArray(0))
                                        }
                                        DucatLog.i(TAG, "took a hail to ${n.dest}")
                                        withContext(Dispatchers.Main) {
                                            MainActivity.openChat.value = rider.personaHex
                                        }
                                    }.onFailure {
                                        DucatLog.w(TAG, "claim: ${it.message}")
                                        error = "Someone beat you to that one."
                                    }
                                    busy = false
                                }
                            },
                            enabled = !busy,
                            modifier = Modifier.fillMaxWidth().height(44.dp),
                        ) { Text("Take this ride") }
                    }
                }
            }
        }

        error?.let {
            Spacer(Modifier.height(10.dp))
            Text(it, color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall)
        }
    }
}

private data class SeenHail(
    val subkey: UInt,
    val card: String,
    val dest: String,
    val farePxmr: Long?,
    val expiry: Long,
)
