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
