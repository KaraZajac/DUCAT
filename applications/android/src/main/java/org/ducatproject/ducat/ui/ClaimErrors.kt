package org.ducatproject.ducat.ui

import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.R
import org.ducatproject.ducat.saidWhy

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
    // Its own sentence, because the fallback below is a list of guesses
    // ("broken, already claimed, or no longer valid") and none of them is
    // true here — the card is fine, it is simply this device's.
    t is Mailbox.OwnCard -> R.string.claim_own_card
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
fun isChainWait(t: Throwable): Boolean = chainWaitBlocks(t) != null

/**
 * How many blocks the chain still wants, when that is the answer.
 *
 * The count was already being read to build the sentence; it is worth keeping
 * because it is also a *duration*. Monero aims at a block every two minutes,
 * so "six more confirmations" is "about twelve minutes" — which is the form
 * of the answer somebody standing there waiting to be paid actually wants.
 */
fun chainWaitBlocks(t: Throwable): Int? =
    NEEDS_CONFIRMATIONS.find(t.message.orEmpty())?.groupValues?.get(1)?.toIntOrNull()

fun moneyFailure(
    context: android.content.Context,
    t: Throwable,
    /**
     * What to say when nothing below matches.
     *
     * Per screen, because "we could not do that" is worse than "that photo
     * would not send" when the screen knows which it was — and the default is
     * the only sentence that fits everywhere.
     */
    fallback: Int = org.ducatproject.ducat.R.string.main_card_link_failed_body,
    /**
     * What to say instead of [fallback] when the throwable is none of the
     * kinds below — for a screen that would rather show the engine's own
     * sentence than a blank one. Null (the default) shows [fallback].
     */
    orElse: (() -> String?)? = null,
): String = when {
    // Our own node, not the Monero one. `claimFailureRes` has said this since
    // the first card that would not claim; the money screens never did, so a
    // release proposed before the routing table was ready reached the person
    // waiting to be paid as the word "TryAgain".
    Mailbox.isOffline(t) -> context.getString(org.ducatproject.ducat.R.string.pay_offline)
    // The three failures in Ceremony a person meets by circumstance rather
    // than by something being broken. Everything else that throws in there is
    // an invariant, and its English sentence is for whoever reads the bug
    // report.
    t is org.ducatproject.ducat.Ceremony.NoNode ->
        context.getString(org.ducatproject.ducat.R.string.pay_node_unreachable)
    t is org.ducatproject.ducat.Ceremony.AlreadyPaid ->
        context.getString(org.ducatproject.ducat.R.string.pay_already_paid)
    // The third: "call it off" tapped in the seconds between the other
    // side's stake landing and this device's scan noticing it.
    t is org.ducatproject.ducat.Ceremony.HoldsMoney -> context.getString(
        org.ducatproject.ducat.R.string.bond_holds_money,
        org.ducatproject.ducat.Amounts.show(context, t.pxmr).primary,
    )
    // §16.18.1: a board notice is stamped against a recent Monero block, so
    // posting one needs a node even though *reading* boards never does. Its
    // own sentence rather than pay_node_unreachable's, which ends "nothing
    // was sent" — nothing was being sent, and a hail is not a payment.
    t is org.ducatproject.ducat.Beacons.NoBlock ->
        context.getString(org.ducatproject.ducat.R.string.board_needs_a_block)
    // The board shows what is near you, and your own posts are near you. The
    // browser filters those out, so meeting this means a card reached the
    // claim some other way — a link sent to yourself, a code scanned off your
    // own screen — and the only useful thing to say is whose card it is.
    t is org.ducatproject.ducat.Mailbox.OwnCard ->
        context.getString(org.ducatproject.ducat.R.string.claim_own_card)
    // Two different answers, deliberately worded apart. One is a wait and the
    // other is an accusation, and a person deciding whether to hand over money
    // needs to know which they are looking at.
    t is org.ducatproject.ducat.Ceremony.EscrowNotConfirmed ->
        context.getString(org.ducatproject.ducat.R.string.escrow_not_confirmed)
    t is org.ducatproject.ducat.Ceremony.EscrowDisagreed ->
        context.getString(org.ducatproject.ducat.R.string.escrow_disagreed)
    // Before node trouble, which it would otherwise match on the timeout
    // quoted inside it: signed and pushed, and no node confirmed. "Nothing
    // was sent" is the one thing this does not know.
    org.ducatproject.ducat.Wallet.relayUnconfirmed(t) ->
        context.getString(org.ducatproject.ducat.R.string.pay_relay_unconfirmed)
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
    // Things a person meets by circumstance rather than by something being
    // broken, and which used to reach them as English through `it.message`.
    t is org.ducatproject.ducat.Mailbox.NoKeysYet ->
        context.getString(org.ducatproject.ducat.R.string.err_no_keys_yet)
    t is org.ducatproject.ducat.Mailbox.ConversationTooOld ->
        context.getString(org.ducatproject.ducat.R.string.err_conversation_too_old)
    t is org.ducatproject.ducat.Hailing.BoardFull ->
        context.getString(org.ducatproject.ducat.R.string.err_board_full)
    else -> orElse?.invoke() ?: context.getString(fallback)
}

/**
 * Cut a card for a screen that cannot do its work without one, and keep
 * cutting until one is cut.
 *
 * The sale screens — the till, the bar tab, the kiosk, the taxi, the
 * donation box — each asked once, on first composition, and showed
 * [moneyFailure]'s sentence when the node was not attached yet: a phone
 * stood up and left, or opened on the way in, wore "offline" for the
 * evening, and the only way to a code was to leave the screen and come
 * back. Backed off to half a minute, because a phone with no network is
 * not helped by asking faster; [onFailure] gets each miss so the screen
 * can say why it is still waiting, and the return is the card.
 *
 * Suspending, not registry-run: a screen that goes takes its card
 * with it (the tap offer, the claim poll, the sale are all its), so the
 * cutting should stop with it too.
 */
suspend fun issueCardPatiently(
    context: android.content.Context,
    validSecs: ULong,
    purpose: String,
    /** [moneyFailure]'s fallback — the screen's own sentence for a miss
     *  that is none of the kinds it names. */
    fallback: Int = org.ducatproject.ducat.R.string.main_card_link_failed_body,
    onFailure: (String) -> Unit,
): org.ducatproject.ducat.IssuedHandle {
    var wait = 5_000L
    while (true) {
        val r = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
            runCatching {
                Mailbox.issueCard(
                    context, org.ducatproject.ducat.MyProfile(context).name(),
                    validSecs, purpose = purpose,
                )
            }
        }
        r.getOrNull()?.let { return it }
        val e = r.exceptionOrNull()!!
        // The class name, for the log: the sentence the screen gets folds
        // every kind of "not yet" into one, and an autopsy wants to know
        // which it was.
        org.ducatproject.ducat.DucatLog.w(
            "Cards", "$purpose card: ${e.javaClass.simpleName}: ${e.message}",
        )
        onFailure(moneyFailure(context, e, fallback))
        kotlinx.coroutines.delay(wait)
        wait = (wait * 2).coerceAtMost(30_000L)
    }
}

/**
 * The registry key a claim of [text] runs under: one per card, so the
 * screen that comes back after a rotation finds the claim it started and
 * does not start another against the same reply slot.
 */
fun claimKey(text: String): String = "claim:" + text.trim()

/**
 * Claim a card where the screen cannot cancel it.
 *
 * Four screens claim a card somebody is holding out — a tapped link, the
 * scanner, the contacts sheet, a listing's "Ask about it" — and each ran
 * the claim in its own scope. The claim itself always finished (the reply
 * subkey written, the contact in the book); it was the line after it that
 * a rotation or a call in those seconds skipped, so the thread it opened
 * was never shown and the card sat on the screen as if nothing had
 * happened. A second tap found the thread (CardAlreadyMine), which is why
 * nobody lost a card — but nobody was told either, and a scanner that
 * came back with `claiming` false read the same code again and raced its
 * own first claim for the one reply slot.
 *
 * Under [ThreadSends] the claim finishes regardless, and whichever
 * instance of the screen is up reads the outcome under [claimKey]: the
 * persona hex of the thread for a landing (see [claimed]), the throwable
 * for a failure ([claimFailureRes] has the words).
 *
 * [onFresh] runs after a first claim only; the same card claimed again
 * from this phone goes to [onAgain] with the thread it already opened.
 * Both run on the job's thread, before the outcome is filed.
 */
fun claimOffScreen(
    context: android.content.Context,
    text: String,
    petname: String? = null,
    onFresh: (org.ducatproject.ducat.Contact) -> Unit = {},
    onAgain: (org.ducatproject.ducat.Contact) -> Unit = {},
) {
    ThreadSends.launch(org.ducatproject.ducat.ContactStore(context), claimKey(text), null) {
        runCatching {
            val card = uniffi.ducat_mobile.readContactCard(text.trim())
            Mailbox.claimCard(context, card, petname).also(onFresh)
        }.recoverCatching { e ->
            ((e as? Mailbox.CardAlreadyMine)?.contact ?: throw e).also(onAgain)
        }.getOrThrow().personaHex
    }
}

/** The thread a landed claim opened, looked up fresh: the outcome carries
 *  the persona hex, and the record may have moved on since. */
fun ThreadSends.Outcome.Landed.claimed(context: android.content.Context): org.ducatproject.ducat.Contact? =
    result?.let { hex ->
        org.ducatproject.ducat.ContactStore(context).all().firstOrNull { it.personaHex == hex }
    }
