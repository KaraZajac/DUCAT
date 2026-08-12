package org.ducatproject.ducat.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import uniffi.ducat_mobile.*

/**
 * Does the native core actually work on this handset?
 *
 * The libraries build, package, and the bindings compile — but none of that
 * exercises JNI marshalling, which is the one part the Rust tests cannot reach.
 * Until this has run on a real device, "the bridge works" is an assertion, and
 * the project's habit is to not make those.
 *
 * So the first build is deliberately a self-test. Each row calls across the
 * bridge and shows what came back; a marshalling fault surfaces as a caught
 * exception with its message rather than as a silent wrong number, because a
 * bridge that returns plausible garbage is worse than one that crashes.
 */
data class Check(val name: String, val expected: String, val got: String, val ok: Boolean)

private fun run(name: String, expected: String, body: () -> String): Check =
    try {
        val got = body()
        Check(name, expected, got, got == expected)
    } catch (t: Throwable) {
        Check(name, expected, "${t::class.simpleName}: ${t.message}", false)
    }

fun bridgeChecks(): List<Check> = listOf(
    // A string crossing the boundary: exercises RustBuffer and UTF-8 decoding.
    run("protocol version", "DUCAT-v1") { protocolVersion() },

    // §17.2, the number the home screen must not overstate. Six unlocked
    // outputs bought four payments in the drain test, so this is the measured
    // answer and not a guess.
    run("capacity of 6 outputs", "4") { approxPaymentsSupported(6u).toString() },
    run("capacity of 1 output", "0") { approxPaymentsSupported(1u).toString() },

    // A record crossing back: two fields, one of them 64-bit.
    run("float plan for 10 payments", "15 outputs") {
        "${planFloat(10u, 2_000_000_000uL).outputs} outputs"
    },

    // §17.8: an exact balance is not a bucket, and the floor rounds **down**.
    // 4.999 XMR sits just under the 5 XMR rung, so it floors to 2 — rounding to
    // nearest would let a bond claim capacity it does not have, and the party
    // who benefits from that overstatement is the one publishing it.
    //
    // The first version of this file expected 5000000000 and was wrong. Caught
    // by asking core rather than by reasoning about the ladder, which is the
    // only reason it is not now a failing check on someone's phone that looks
    // like a bridge fault.
    run("bucket floor of 4.999 XMR", "2000000000") {
        capacityBucket(4_999_999_999uL).toString()
    },

    // §15.5.1's rule that costs most to get backwards: a stale rate escalates.
    run("stale rate escalates", "AppSecret") {
        checkVerification(
            defaultVerificationPolicy(),
            deviceUnlocked = true,
            appSecretAgeS = null,
            amountMinor = 1uL,
            spentInWindowMinor = 0uL,
            rateIsFresh = false,
        ).required.name
    },
)

@Composable
fun BridgeSelfTest() {
    val checks = remember { bridgeChecks() }
    val failed = checks.count { !it.ok }

    Card(
        Modifier.fillMaxWidth().padding(16.dp),
        shape = RoundedCornerShape(16.dp),
    ) {
        Column(Modifier.padding(16.dp)) {
            Text(
                if (failed == 0) "Native core: ${checks.size} checks passed"
                else "Native core: $failed of ${checks.size} FAILED",
                style = MaterialTheme.typography.titleMedium,
                color = if (failed == 0) MaterialTheme.colorScheme.primary
                        else MaterialTheme.colorScheme.error,
            )
            Spacer(Modifier.height(4.dp))
            Text(
                "These call into the Rust core across JNI — the one layer the " +
                    "Rust tests cannot reach.",
                style = MaterialTheme.typography.bodySmall,
            )
            Spacer(Modifier.height(12.dp))
            checks.forEach { c ->
                Row(Modifier.fillMaxWidth().padding(vertical = 3.dp)) {
                    Text(if (c.ok) "✓ " else "✗ ")
                    Column {
                        Text(c.name, style = MaterialTheme.typography.bodyMedium)
                        Text(
                            if (c.ok) c.got else "got ${c.got}, expected ${c.expected}",
                            style = MaterialTheme.typography.bodySmall,
                            fontFamily = FontFamily.Monospace,
                            color = if (c.ok) MaterialTheme.colorScheme.onSurfaceVariant
                                    else MaterialTheme.colorScheme.error,
                        )
                    }
                }
            }
        }
    }
}
