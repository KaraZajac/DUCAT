package org.ducatproject.ducat

import android.content.Context
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private const val TAG = "DucatPoller"

/**
 * Collects what has arrived — woken by the network, swept on a timer.
 *
 * This replaces the responder that answered `app_call`s: a message written
 * while this phone was off waits in a record rather than dying with a route.
 * The original build polled on a fixed interval and said watches were "worth
 * doing and deliberately not done first: a poll is correct whether or not a
 * watch survives backgrounding". That condition is now honoured rather than
 * retired — **the sweep stays**, identical, every interval, and watches only
 * decide how early the next pass starts. A watch that dies, expires, or was never
 * placed costs latency, never a message.
 */
class Poller(private val context: Context) {

    /** Probe the candidates and remember the first usable one. */
    private fun pickNode(context: Context): String? = try {
        val store = NodeStore(context)
        val s = uniffi.ducat_mobile.moneroPickNode(
            uniffi.ducat_mobile.moneroDefaultNodes(store.ownUrl()),
            "stagenet",
            8_000u,
        )
        store.rememberLastGood(s.url)
        DucatLog.i(TAG, "picked node ${s.url} at height ${s.height}")
        s.url
    } catch (e: Exception) {
        DucatLog.w(TAG, "no node: ${e.message}")
        null
    }

    fun start(scope: CoroutineScope) {
        scope.launch(Dispatchers.IO) {
            // The pace follows the screen (the roadmap's battery tier): while
            // someone is looking, every wake below runs the full sweep — the
            // hot path is exactly what it always was. In a pocket, a wake
            // sweeps only when a watch rang (a message deserves its
            // notification promptly) or on a heartbeat, so a quiet phone does
            // roughly one sweep every three minutes instead of five a minute.
            // The wait stays short in both tiers: a ring or a return to the
            // foreground is answered within one chunk, never one sleep.
            var rang = true // the first pass sweeps: boot is "what did we miss"
            var quietWakes = 0
            while (isActive) {
                val fg = AppVisibility.foreground
                if (!fg && !rang && quietWakes < BG_HEARTBEAT_WAKES) {
                    quietWakes++
                    rang = withContext(Dispatchers.IO) {
                        runCatching { uniffi.ducat_mobile.nodeWaitChange(WAIT_MS) }
                            .getOrDefault(false)
                    }
                    continue
                }
                if (!fg) {
                    DucatLog.i(
                        TAG,
                        if (rang) "background sweep — a watch rang"
                        else "background heartbeat sweep",
                    )
                }
                quietWakes = 0

                // The transport's own narration (attach progress, dial
                // failures), which otherwise has nowhere to go on a phone.
                runCatching {
                    uniffi.ducat_mobile.nodeLogs().forEach { DucatLog.i("veilid", it) }
                }

                runCatching {
                    // Answers to a card we handed out land in its inbox, and
                    // that only becomes a contact once somebody looks.
                    Mailbox.collectClaims(context)
                    // A rental card is cut for one listing, so whoever
                    // answered one is asking about that listing and no other.
                    // Here rather than inside the mailbox, which has no
                    // business knowing that renting exists.
                    runCatching { Listings.linkClaims(context) }
                    val n = Mailbox.poll(context)
                    if (n > 0) DucatLog.i(TAG, "collected $n message(s)")
                }.onFailure { DucatLog.w(TAG, "poll: ${it.message}") }

                // The chain, in windows. Kept on the same loop as messages
                // because both are "what happened while we were not looking",
                // and a second timer would just be another thing to get wrong.
                runCatching {
                    // Find a node ourselves rather than waiting for a screen to
                    // do it. This used to read whatever the Status panel had
                    // last stored, which meant the wallet did not sync until the
                    // user happened to open that screen — a background job that
                    // depends on a screen having run is not a background job.
                    val node = NodeStore(context).lastGood() ?: pickNode(context)
                    if (node != null) {
                        val before = WalletStore(context).entries().map { it.keyImage }.toSet()
                        val moved = Wallet.scanStep(context, node)
                        if (moved) {
                            Wallet.refreshSpent(context, node)
                            // Money that arrived while nobody was looking. Only
                            // during steady state — a wallet mid-catch-up would
                            // fire one notification per historical receipt.
                            val b = Wallet.balances(context)
                            if (!b.syncing) {
                                // Change is not income — the Ledger's oldest
                                // lesson, relearned here: the remainder of your
                                // own send comes back as a new output, and
                                // announcing it as "money arrived" told someone
                                // who had just paid that they had been paid.
                                // Every output knows its transaction, and a
                                // transaction in our send records is ours.
                                val ourSends = WalletStore(context).sends()
                                    .map { it.txidHex.lowercase() }.toSet()
                                WalletStore(context).entries()
                                    .filter { it.keyImage.isNotEmpty() && it.keyImage !in before }
                                    .forEach {
                                        if (it.txHashHex.lowercase() in ourSends) {
                                            DucatLog.i(
                                                TAG,
                                                "change back: ${formatXmr(it.amountPxmr)} XMR",
                                            )
                                        } else {
                                            Notify.post(
                                                context, "Money arrived",
                                                "${formatXmr(it.amountPxmr)} XMR — unlocks after ten blocks",
                                            )
                                        }
                                    }
                            }
                        }
                        // Turn outputs back into transactions: which ones we
                        // sent, and when each block was mined. A few per pass,
                        // because each is a round trip and this loop also has
                        // to stay responsive.
                        Ledger.enrich(context, node)
                    }
                }.onFailure { DucatLog.w(TAG, "scan: ${it.message}") }

                // Settled tabs and fares: payment seen on chain, receipt into
                // the thread. Here rather than on a screen, because the payment
                // lands when it lands and the vendor is busy (§15.11).
                runCatching { TabStore.reconcile(context) }
                    .onFailure { DucatLog.w(TAG, "tabs: ${it.message}") }
                // The mempool, only while a bill is out and unsighted — the
                // scan costs a round trip per pool transaction, and a till
                // with nothing billed has nothing to look for.
                runCatching {
                    NodeStore(context).lastGood()?.let { TabStore.poolSight(context, it) }
                }.onFailure { DucatLog.w(TAG, "pool: ${it.message}") }

                // Stewardship (§18.7): a good tenant cleans its own unit.
                // Cards whose purpose is spent leave the registry, and this
                // device stops holding their records; the network's copies
                // expire by TTL on their own.
                runCatching {
                    ContactStore(context).pruneCards().forEach {
                        runCatching { uniffi.ducat_mobile.nodeDhtDelete(it) }
                    }
                }.onFailure { DucatLog.w(TAG, "prune: ${it.message}") }

                // One attachment per pass: pictures arrive shortly after
                // their messages without ever starving the payment paths.
                runCatching { Mailbox.fetchOneAttachment(context) }
                    .onFailure { DucatLog.w(TAG, "attachment: ${it.message}") }

                // Cheap: it only leaves the device when the cache has expired.
                runCatching { Rates.refresh(context) }
                    .onFailure { DucatLog.w(TAG, "rate: ${it.message}") }

                // Re-arm watches on what can ring: every contact's log head
                // (their next message moves it) and every unanswered card's
                // inbox (a claim writes it). Re-armed each pass because
                // watches expire and the network only promises best effort —
                // the sweep below is still the guarantee.
                runCatching {
                    val store = ContactStore(context)
                    store.all().forEach {
                        runCatching { uniffi.ducat_mobile.nodeDhtWatch(it.theirOutbox) }
                    }
                    store.issuedCards().filter { it.answeredBy == null }.forEach {
                        runCatching { uniffi.ducat_mobile.nodeDhtWatch(it.inboxKey) }
                    }
                }.onFailure { DucatLog.w(TAG, "watch: ${it.message}") }

                // Sleep until the network rings or the interval passes —
                // instant messages when watches hold, the old cadence when
                // they do not.
                rang = withContext(Dispatchers.IO) {
                    runCatching { uniffi.ducat_mobile.nodeWaitChange(WAIT_MS) }.getOrDefault(false)
                }
                if (rang) {
                    // Which records, drained here for the same reason the flag
                    // is consumed here: these are events, and whoever asks
                    // gets them. Handed on rather than used — this loop polls
                    // everything anyway; the stand sweep is the one that can
                    // save eighteen board reads by knowing which one moved.
                    val moved = runCatching { uniffi.ducat_mobile.nodeChangedKeys() }
                        .getOrDefault(emptyList())
                    DucatLog.i(TAG, "a watched record changed — polling now")
                    // Pass the ring on. `node_wait_change` *consumes* the flag
                    // — whoever wakes first clears it — so there can only be
                    // one caller of it in the process, and this is it. Anything
                    // else that wants to know the network rang (the driver's
                    // stand sweep, which would otherwise poll blind on a timer)
                    // listens here instead of racing the poller for its
                    // wake-ups and delaying somebody's messages to do it.
                    NetworkRings.note(moved)
                }
            }
        }
    }

    private companion object {
        /** One wait chunk. Short in both tiers so a ring or a return to the
         *  foreground is answered within a chunk, never within a sleep. */
        const val WAIT_MS = 10_000u

        /** Quiet background wakes between heartbeat sweeps: 18 × 10 s ≈ 3
         *  minutes. Watches make messages instant regardless; the heartbeat
         *  is the guarantee behind them, exactly like the sweep itself. */
        const val BG_HEARTBEAT_WAKES = 18
    }
}
