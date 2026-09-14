package org.ducatproject.ducat.ui

import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import org.ducatproject.ducat.Amounts
import org.ducatproject.ducat.R
import org.ducatproject.ducat.Trust

/**
 * §9.5's badge, in words, wherever a persona stands at a decision — the
 * thread's header, a listing's poster, a driver's offer, a till's customer
 * row. One builder, so the four screens cannot drift: "Burned 0.01 XMR,
 * since block N" from what this phone verified itself, "3 receipts, 1 from
 * burned personas · 4.7 ★" from the record it read — the star average only
 * over signers whose burn it checked, which is the one number here that
 * means anything — and §9.2's "Pat and Sam know them" over its own
 * contacts. Null when nothing is known, so a stranger's header says
 * nothing rather than "burned nothing".
 *
 * [sayNone] is the listing's exception (the desk's `Market.svelte`): on a
 * poster, "No burn of theirs checked here" is said in the burn's place,
 * because there the silence would read as an oversight rather than as the
 * fact it is.
 */
@Composable
internal fun trustBadge(
    burn: Trust.VerifiedBurn?,
    record: Trust.RecordSummary,
    knownBy: List<String>,
    sayNone: Boolean = false,
): String? {
    val context = LocalContext.current
    val parts = ArrayList<String>(3)
    if (burn != null) {
        parts += stringResource(
            R.string.trust_burned_since,
            Amounts.show(context, burn.amountPxmr).primary,
            Amounts.count(burn.height),
        )
    } else if (sayNone) {
        parts += stringResource(R.string.trust_no_burn_known)
    }
    if (record.receipts > 0) {
        var line = stringResource(
            R.string.trust_receipts_summary,
            Amounts.count(record.receipts.toLong()),
            Amounts.count(record.weighted.toLong()),
        )
        if (record.weighted > 0) line += " · " + "%.1f".format(record.ratingX10 / 10.0) + " ★"
        parts += line
    }
    // §9.2: who among this phone's contacts knows them, in words — and
    // nothing when none do, because "nobody you know knows them" is what
    // every stranger's header would say.
    knownWords(knownBy)?.let { parts += it }
    return if (parts.isEmpty()) null else parts.joinToString(" · ")
}

/** The same, over what [Trust.badgeOf] or [Trust.badgesOf] read. */
@Composable
internal fun trustBadge(b: Trust.Badge, sayNone: Boolean = false): String? =
    trustBadge(b.burn, b.record, b.knownBy, sayNone)

/**
 * §9.2's answer worn in words: "Pat knows them", "Pat and Sam know them",
 * "3 of your contacts know them" — over the names [Trust.knownBy] found
 * among this phone's own contacts. Null when none. Never a score, never
 * the hex, and never anyone this phone does not already hold.
 */
@Composable
internal fun knownWords(names: List<String>): String? = when (names.size) {
    0 -> null
    1 -> stringResource(R.string.trust_known_by_one, names[0])
    2 -> stringResource(
        R.string.trust_known_by_names,
        names.joinToString(stringResource(R.string.trust_and)),
    )
    else -> stringResource(R.string.trust_known_by_count, Amounts.count(names.size.toLong()))
}
