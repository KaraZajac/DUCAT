package org.ducatproject.ducat.ui

import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.R

/**
 * Why a card could not be claimed, in words the reader can act on.
 *
 * Five screens claim cards — a tapped link, the scanner, a listing, a hail a
 * driver takes, and the contacts screen — and each used to explain a failure
 * its own way. Three of them printed `exception.message`, which is how typing
 * these failures made things *worse* before it made them better: the English
 * sentence that used to live inside the throw became "card already claimed",
 * and those screens started showing it.
 *
 * The distinctions are worth keeping because they lead to different actions:
 *
 *  - **offline** — the node has not finished joining. The card is fine. Wait.
 *  - **already used** — a card is good for one person (§7.5). Ask for another.
 *  - **not published yet** — the card is real but its details have not landed;
 *    a till mints one per sale and a customer can outrun the write. Scan again.
 *  - anything else — the honest three maybes.
 *
 * [alreadyUsed] exists because one caller means something specific by it: a
 * driver reaching for a hail that another driver already took has not been
 * handed a stale card, they have lost a race, and that is what to say.
 */
fun claimFailureRes(
    t: Throwable,
    alreadyUsed: Int = R.string.contacts_reply_replay,
): Int = when {
    Mailbox.isOffline(t) -> R.string.main_card_link_offline
    t is Mailbox.CardAlreadyUsed -> alreadyUsed
    t is Mailbox.DetailsNotPublished -> R.string.main_card_link_not_ready
    else -> R.string.main_card_link_failed_body
}

/**
 * A failure from something that needed the network, in words.
 *
 * The escrow screens showed `it.message` verbatim, so a node that did not
 * answer in time reached a person as
 * `v1=decoys: InterfaceError(InterfaceError("timed out"))` — on the screen
 * holding their fare. The wallet already knows that shape of failure
 * (`Wallet.isNodeTrouble`) and the Pay screen already has the sentence for
 * it; only these screens were not using either.
 *
 * Named for money once, because escrow is where it was first needed. The
 * shape has nothing to do with money: posting a listing, sending a message,
 * publishing a card and taking a hail all reach the same node over the same
 * transport and fail the same way, and every one of them was printing the
 * same unreadable line at somebody. Same sentence for all of them.
 */
/**
 * How many confirmations a release is still waiting on, if that is the answer.
 *
 * The core refuses a release whose newest escrow output is younger than
 * Monero's ten blocks, and says so with a count. On a slow chain that is the
 * common case rather than the corner — so it is a sentence real users read,
 * and it was reaching them as raw English from a Rust `format!`, in an app
 * that ships in nineteen languages.
 *
 * Matched on the count rather than the wording, and the wording is the
 * fallback when the match misses. A regex over an error string is not
 * elegant; shipping one language to everybody is worse.
 */
private val NEEDS_CONFIRMATIONS = Regex("""needs (\d+) more confirmation""")

/**
 * Is this the chain saying "not yet" rather than something being wrong?
 *
 * Asked of the throwable, because the sentence is no longer in a language
 * anyone can grep for. Two screens decided how alarming to look by testing the
 * message for the word "confirmation" — correct while the message was raw
 * English from a Rust `format!`, and wrong the moment it became a localised
 * plural: in the other eighteen languages the word is not there, so the
 * ordinary "wait six blocks" answer turned red and read as a failure.
 */
fun isChainWait(t: Throwable): Boolean =
    NEEDS_CONFIRMATIONS.containsMatchIn(t.message.orEmpty())

fun moneyFailure(context: android.content.Context, t: Throwable): String = when {
    org.ducatproject.ducat.Wallet.isNodeTrouble(t) ->
        context.getString(org.ducatproject.ducat.R.string.pay_node_no_answer)
    // Short, with both numbers. Typed rather than matched on wording, because
    // this one is thrown by our own Kotlin and there is no reason to write a
    // sentence in order to read it back with a regex.
    t is org.ducatproject.ducat.Wallet.NotEnough -> context.getString(
        org.ducatproject.ducat.R.string.pay_not_enough,
        org.ducatproject.ducat.Amounts.show(context, t.availablePxmr).primary,
        org.ducatproject.ducat.Amounts.show(context, t.neededPxmr).primary,
    )
    NEEDS_CONFIRMATIONS.find(t.message.orEmpty()) != null -> {
        val n = NEEDS_CONFIRMATIONS.find(t.message.orEmpty())!!.groupValues[1].toInt()
        context.resources.getQuantityString(
            org.ducatproject.ducat.R.plurals.bond_needs_confirmations, n, n,
        )
    }
    else -> bridgeMessage(t.message ?: context.getString(
        org.ducatproject.ducat.R.string.main_card_link_failed_body,
    ))
}
