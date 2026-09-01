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

    /** The last transport state narrated, so only changes are. */
    private var lastAttach: String? = null

    /** When the node was last coaxed back to life — see the restart below. */
    private var lastRevive = 0L

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
        scope.launch(Dispatchers.IO) { lane(scope) }
        scope.launch(Dispatchers.IO) {
            // The sweep is a timer, nothing more: fast in the foreground,
            // a heartbeat in a pocket. Messages no longer ride it — the
            // lane thread answers rings the moment they land, mid-pass or
            // not — so the sweep is back to being what it always claimed:
            // the correctness pass behind the push.
            while (isActive) {
                if (!AppVisibility.foreground) {
                    DucatLog.i(TAG, "background heartbeat sweep")
                }

                // The transport's own narration (attach progress, dial
                // failures), which otherwise has nowhere to go on a phone.
                runCatching {
                    uniffi.ducat_mobile.nodeLogs().forEach { DucatLog.i("veilid", it) }
                }

                // Where the transport has got to, each time that changes.
                //
                // The ring above stays empty on veilid-core 0.5.7 — api-level
                // logging goes through a tracing layer nobody has installed
                // (see node.rs) — so a node still finding its first peer and a
                // node that failed to start read identically from the log:
                // both just say the mailbox is offline, once a pass, forever.
                // Ten minutes of that during a two-phone run was
                // indistinguishable from broken, and the answer was sitting in
                // node_status the whole time.
                runCatching {
                    val s = uniffi.ducat_mobile.nodeStatus()
                    val now = if (s.running) s.state else "not started"
                    if (now != lastAttach) {
                        lastAttach = now
                        DucatLog.i(TAG, "transport $now — ${s.peers} peer(s)")
                    }

                    // A node that failed to start will not start itself.
                    //
                    // Start is attempted exactly once, at launch, and its
                    // result is discarded on the reasoning that nothing can
                    // act on it. Something can: this loop. Caught on the
                    // emulator after a reinstall, where the app came up with
                    // the node dead and simply stayed that way — every board
                    // read returned nothing, and the search offered "try
                    // again in a moment", which was a promise the app had no
                    // way to keep. Only a force-quit fixed it.
                    //
                    // Cheap to be wrong about: node_start on a node that is
                    // already up returns success without doing anything, so
                    // the worst case of a spurious call is a lock and a
                    // comparison.
                    val at = System.currentTimeMillis()
                    if (!s.running && at - lastRevive > REVIVE_EVERY_MS) {
                        lastRevive = at
                        DucatLog.w(TAG, "transport is down — starting the node again")
                        runCatching {
                            uniffi.ducat_mobile.nodeStart(
                                "${context.filesDir.absolutePath}/veilid",
                                udp = true,
                            )
                        }.onFailure {
                            DucatLog.w(
                                TAG,
                                "restart: ${it.javaClass.simpleName}: ${it.message}",
                            )
                        }
                    }
                }

                // Keep what is on the boards on them.
                //
                // A notice carries a 24-hour expiry and `needRefresh` returns
                // the ones past six hours so they can be re-posted before it
                // runs out. Nothing in the app ever called it — only the desk
                // test harness did — so every listing quietly fell off the
                // board a day after it went up, while the owner's screen went
                // on saying "Live on the board near you" because that label
                // reads a local flag set at posting and never cleared.
                //
                // Re-posting is also where a listing's price is brought back
                // in line with the currency it was written in (see
                // Listings.reprice), so this was two features waiting on one
                // loop that did not exist.
                runCatching {
                    Listings.needRefresh(context).forEach { l ->
                        runCatching { Listings.post(context, l.optString("id")) }
                            .onFailure { DucatLog.w(TAG, "refresh listing: ${it.message}") }
                    }
                }.onFailure { DucatLog.w(TAG, "listing refresh: ${it.message}") }

                // A turn nobody has taken, mentioned again — see
                // Ceremony.remindWaiting. Cheap: it reads records already on
                // disk and sends nothing unless an hour has passed.
                runCatching { Ceremony.remindWaiting(context) }
                    .onFailure { DucatLog.w(TAG, "reminders: ${it.message}") }

                // Disappearing messages, for the threads nobody is looking at.
                // The chat screen applies the window on the conversation it has
                // open, which is the one case where it hardly matters; see
                // ContactStore.expireAll.
                runCatching {
                    val gone = ContactStore(context).expireAll()
                    if (gone > 0) DucatLog.i(TAG, "$gone message(s) past their window")
                }.onFailure { DucatLog.w(TAG, "retention: ${it.message}") }

                runCatching {
                    // Messages first: somebody may be waiting on one, and the
                    // card sweep ahead of this cost a renewal tick 54 seconds
                    // once — claims keep, people don't.
                    val n = Mailbox.poll(context)
                    if (n > 0) {
                        DucatLog.i(TAG, "collected $n message(s)")
                        runCatching { Calls.noticed(context) }
                    }
                    // Answers to a card we handed out land in its inbox, and
                    // that only becomes a contact once somebody looks.
                    Mailbox.collectClaims(context)
                    // A rental card is cut for one listing, so whoever
                    // answered one is asking about that listing and no other.
                    // Here rather than inside the mailbox, which has no
                    // business knowing that renting exists.
                    runCatching { Listings.linkClaims(context) }
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
                                val ourSends = WalletStore(context).ourTxids()
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
                                                context,
                                                context.getString(
                                                    R.string.notify_arrived_title,
                                                ),
                                                context.getString(
                                                    R.string.notify_arrived_body,
                                                    Amounts.show(
                                                        context, it.amountPxmr,
                                                    ).primary,
                                                ),
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
                // Donation threads get their receipts on the same clock the
                // tabs do — see Donations for why the payer wants the record.
                runCatching { Donations.reconcile(context) }
                    .onFailure { DucatLog.w(TAG, "tabs: ${it.message}") }
                // Paid subscribers get their issue on the same clock too:
                // the tab just went "paid" above, and §15.11 says delivery
                // follows settlement, not the operator's attention.
                runCatching { Publications.reconcileSettled(context) }
                    .onFailure { DucatLog.w(TAG, "issues: ${it.message}") }
                // And the shelf stays alive — hourly inside, cheap here.
                runCatching { Publications.tendShelf(context) }
                // Market tenancies re-post past half their TTL or a weekly
                // generation rollover, minting a fresh claim-once each time.
                runCatching { Publications.tendMarket(context) }
                // The invisible help: the boards around the last fix, and
                // the shelf category last read, warmed before anyone opens
                // a browse screen. See warmBoards for its gates.
                runCatching { warmBoards(context, scope) }
                // The club keeps its books on the shelf: once per process,
                // when the node is up, every swarm-shipped issue already on
                // this device goes back to serving under its original share
                // key — a verify-only fetch that downloads nothing and
                // stays. Readers are mirrors (§16.20).
                runCatching { reseedLibrary(context) }
                // And the outbox: files this phone sent down the big road
                // stay served for a week — the sender is the first seeder,
                // and a restart must not orphan a transfer the other side
                // has not collected yet. Verify-only, downloads nothing.
                runCatching { reseedOutbox(context) }
                // And the sites somebody promised to keep alive: the park
                // dies with the process, so every restart re-verifies the
                // kept bundles and puts them back on the wire. Without this
                // the checkbox only means "until my next reboot".
                runCatching { reseedSites(context) }
                // And the staged issues nothing will publish (see
                // Publications.sweepStaging): once per process is plenty.
                runCatching { sweepStaging(context) }
                // The mempool, only while a bill is out and unsighted — the
                // scan costs a round trip per pool transaction, and a till
                // with nothing billed has nothing to look for.
                runCatching {
                    NodeStore(context).lastGood()?.let {
                        TabStore.poolSight(context, it)
                        // A kiosk customer is nobody's contact, so their
                        // payment arrives with no thread to announce it in;
                        // the mempool is the only place it shows up before a
                        // block does.
                        Orders.poolSight(context, it)
                    }
                    runCatching { Orders.reconcile(context) }
                    // And stop looking for the ones that walked away, which is
                    // what keeps the sweep above from running all day.
                    runCatching { Orders.expire(context) }
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

                // Before the sweep, not after: a half-built escrow gets its
                // frames sent again while there is still a record to send
                // them from. A dropped ceremony round is otherwise permanent
                // — nothing in the key ceremony ever asked twice.
                runCatching { Ceremony.nudge(context) }
                    .onFailure { DucatLog.w(TAG, "escrow nudge: ${it.message}") }

                // And the other half of a 2-of-3: a release this device did
                // not sign leaves no trace on the escrow address it can read,
                // only an arrival in its own wallet.
                runCatching { Ceremony.checkSettled(context) }
                    .onFailure { DucatLog.w(TAG, "escrow settled: ${it.message}") }

                // §15.12: a ride that has ended takes its position stream with
                // it. Here rather than on a screen, because the bound must
                // hold with the phone in a pocket — which is exactly when a
                // stream left running would be worst.
                // §16.19: group copies that did not land, replayed until they
                // do — the queue is what makes "reached everyone" true late
                // rather than false forever.
                runCatching { Groups.retryOutbox(context) }
                    .onFailure { DucatLog.w(TAG, "group retry: ${it.message}") }
                // Recurring bills: the asking repeats, the paying never does
                // (§16.13 — a request carries no authority). Here for the
                // same reason as the rest: rent comes due with the phone in
                // a drawer.
                runCatching { Recurring.runDue(context) }
                    .onFailure { DucatLog.w(TAG, "recurring: ${it.message}") }
                // §16.12 repair: a write the node accepted with the network
                // dark stays local for ever, while the head advertising it
                // travels — wedging the reader on a slot that never comes.
                // Verify the trailing writes against the network's copy and
                // re-flood our own bytes where they differ.
                runCatching { Mailbox.verifyLastWrites(context) }
                    .onFailure { DucatLog.w(TAG, "slot verify: ${it.message}") }
                runCatching { Positions.enforceBounds(context) }
                    .onFailure { DucatLog.w(TAG, "position stop: ${it.message}") }

                // "It will try again on its own": here, so that it does even
                // when the driver has put the phone away — which, eighteen
                // minutes into waiting for the chain, is what a driver does.
                runCatching { Ceremony.retryRelease(context) }
                    .onFailure { DucatLog.w(TAG, "release retry: ${it.message}") }

                // The same tenancy, for the store that never had a sweep.
                runCatching { Ceremony.sweep(context) }
                    .onFailure { DucatLog.w(TAG, "escrow sweep: ${it.message}") }

                // And for the directory that only ever grew. A chat set to
                // forget its messages kept every picture in them.
                runCatching { Mailbox.sweepAttachments(context) }
                    .onFailure { DucatLog.w(TAG, "attachment sweep: ${it.message}") }

                // Board authors whose listings are long gone. Kept forever it
                // would be a record of everywhere this phone has browsed.
                runCatching { Posters.sweep(context, System.currentTimeMillis()) }
                    .onFailure { DucatLog.w(TAG, "poster sweep: ${it.message}") }

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
                // the sweep below is still the guarantee. Cheap to repeat:
                // veilid keeps desired-state and only renegotiates on change.
                // Health is narrated when it changes, because a silently
                // failed arm reads identically to a working watch until a
                // message is late (push v2, PUSH.md).
                runCatching {
                    val store = ContactStore(context)
                    var up = 0
                    var down = 0
                    store.all().forEach {
                        runCatching { uniffi.ducat_mobile.nodeDhtWatch(it.theirOutbox) }
                            .onSuccess { ok -> if (ok) up++ else down++ }
                            .onFailure { down++ }
                    }
                    store.issuedCards().filter { it.answeredBy == null }.forEach {
                        runCatching { uniffi.ducat_mobile.nodeDhtWatch(it.inboxKey) }
                            .onSuccess { ok -> if (ok) up++ else down++ }
                            .onFailure { down++ }
                    }
                    if (up != watchesUp || down != watchesDown) {
                        watchesUp = up
                        watchesDown = down
                        DucatLog.i(TAG, "watches: $up armed, $down not")
                    }
                }.onFailure { DucatLog.w(TAG, "watch: ${it.message}") }

                // Sleep until the network rings or the interval passes —
                // instant messages when watches hold, the old cadence when
                // they do not.
                // Pace: the screen gets a pass every chunk, the pocket
                // every heartbeat — and a return to the foreground is
                // answered within one chunk, never one sleep.
                var waited = 0L
                while (isActive) {
                    delay(CHUNK_MS)
                    waited += CHUNK_MS
                    if (AppVisibility.foreground || waited >= HEARTBEAT_MS) break
                }
            }
        }
    }

    /**
     * The push lane (PUSH.md): the one consumer of `node_wait_change` in
     * the process — the flag is consumed by whoever wakes first, so there
     * can only be one, and it is this thread, whose whole job is to turn a
     * ring into a targeted read in about a second. It runs beside the
     * sweep, not inside it: the old loop answered rings between passes,
     * which meant a ring during a slow pass waited for the pass — measured
     * at 53 s where this lane measures ~1 s. Boards, rails and streams get
     * their ring through [NetworkRings], exactly as before.
     */
    // ------------------------------------------------------------------
    // The board warmer. A browse screen that opens onto a spinner is the
    // network's latency worn on the sleeve; this pays it early, quietly,
    // so the screen opens onto the neighbourhood instead. Gates, in order:
    // foreground only (a pocketed phone spends nothing), at most once per
    // warmEveryMs, node attached (an unattached read is a 21-second lie),
    // permission granted, and a fix the phone already holds — passiveFix
    // never wakes a radio, so a phone that has not moved or navigated
    // lately simply skips the warm rather than lighting up GPS.
    private val warming = java.util.concurrent.atomic.AtomicBoolean(false)
    @Volatile private var lastWarm = 0L
    @Volatile private var lastShelfWarm = 0L
    private val warmEveryMs = 7 * 60_000L
    private val shelfWarmEveryMs = 15 * 60_000L

    @Volatile private var reseeded = false

    private fun reseedLibrary(context: Context) {
        if (reseeded) return
        if (!runCatching { uniffi.ducat_mobile.nodeStatus().publicInternetReady }
                .getOrDefault(false)
        ) {
            return
        }
        reseeded = true
        // The bound that keeps mirroring polite: only the newest issues per
        // publication go back to serving. Each seed is a live share
        // announcer with routes to maintain, so "re-serve everything ever
        // fetched" would slowly turn every phone into a heavy seed-box —
        // and demand concentrates on recent issues anyway. Older ones stay
        // fetchable from the publisher and the shelf. Publications only:
        // their swarm payloads are sealed under §16.20 period keys, so a
        // mirror holds ciphertext; pairwise transfers (when they arrive)
        // will not re-serve at all.
        val newestPerPub = 2
        for (pub in Publications.subscribedPublishers(context)) {
            if (Publications.isMuted(context, pub)) continue
            val sub = Publications.subscription(context, pub) ?: continue
            for (period in sub.third.keys.sortedDescending().take(newestPerPub)) {
                val ship = Publications.shipment(context, pub, period) ?: continue
                val job = org.ducatproject.ducat.ui.LibraryFetch.Job(pub, period)
                val done = org.ducatproject.ducat.ui.LibraryFetch
                    .fetchedBytes(context, pub, period)
                if (done != null) {
                    org.ducatproject.ducat.ui.LibraryFetch.reseed(
                        context, job,
                        org.ducatproject.ducat.ui.LibraryFetch.Source.Swarm(
                            ship.first, ship.second,
                        ),
                    )
                }
            }
        }
    }

    @Volatile private var outboxReseeded = false

    private fun reseedOutbox(context: Context) {
        if (outboxReseeded) return
        if (!runCatching { uniffi.ducat_mobile.nodeStatus().publicInternetReady }
                .getOrDefault(false)
        ) {
            return
        }
        outboxReseeded = true
        val root = java.io.File(context.filesDir, "swarm_out")
        val weekAgo = System.currentTimeMillis() / 1000 - 7 * 24 * 3600
        root.listFiles()?.forEach { dir ->
            runCatching {
                val meta = org.json.JSONObject(
                    java.io.File(dir, "share.json").readText(),
                )
                if (meta.optLong("sent") < weekAgo) {
                    // A week unserved is a transfer nobody is coming for.
                    dir.deleteRecursively()
                    return@forEach
                }
                Thread {
                    runCatching {
                        Swarm.fetch(
                            meta.getString("share"),
                            meta.getString("digest"),
                            dir.absolutePath,
                            staySeeding = true,
                        )
                    }.onFailure {
                        DucatLog.w(TAG, "outbox reseed: ${it.message}")
                    }
                }.apply { isDaemon = true; name = "outbox-reseed" }.start()
            }
        }
    }

    @Volatile private var stagingSwept = false

    private fun sweepStaging(context: Context) {
        if (stagingSwept) return
        stagingSwept = true
        val freed = Publications.sweepStaging(context)
        if (freed > 0) DucatLog.i(TAG, "swept ${freed / 1024} KiB of staged issues nothing will publish")
    }

    @Volatile private var sitesReseeded = false

    private fun reseedSites(context: Context) {
        if (sitesReseeded) return
        if (!runCatching { uniffi.ducat_mobile.nodeStatus().publicInternetReady }
                .getOrDefault(false)
        ) {
            return
        }
        sitesReseeded = true
        for (site in Sites.all(context)) {
            if (site.keepAlive) Sites.reseed(context, site.recordKey)
        }
    }

    private fun warmBoards(context: Context, scope: CoroutineScope) {
        if (!AppVisibility.foreground) return
        val now = System.currentTimeMillis()
        if (now - lastWarm < warmEveryMs) return
        if (!runCatching { uniffi.ducat_mobile.nodeStatus().publicInternetReady }
                .getOrDefault(false)
        ) {
            return
        }
        if (!org.ducatproject.ducat.ui.locationAllowed(context)) return
        val fix = org.ducatproject.ducat.ui.passiveFix(context, 30 * 60_000L) ?: return
        if (!warming.compareAndSet(false, true)) return
        lastWarm = now
        scope.launch(Dispatchers.IO) {
            try {
                DucatLog.i(TAG, "warming the boards around the last fix")
                // The same wave a browse runs, results discarded: the point
                // is the cache it fills on the way through.
                runCatching { Listings.search(context, fix.first, fix.second, null, {}) }
                // And the worldwide shelf somebody last had open, on its own
                // slower clock — one category, not the whole market.
                if (now - lastShelfWarm >= shelfWarmEveryMs) {
                    lastShelfWarm = now
                    val prefs = context.getSharedPreferences("ducat_market_cache", 0)
                    val cat = prefs.getString("last_cat", null)
                    if (cat != null) {
                        val lang = prefs.getString("last_lang", "")?.ifBlank { null }
                        runCatching { Publications.browseMarket(context, cat, lang) }
                    }
                }
            } finally {
                warming.set(false)
            }
        }
    }

    private fun lane(scope: CoroutineScope) {
        while (scope.isActive) {
            // A wait that throws instead of waiting would spin this thread
            // flat out; pause where the wait would have.
            val rang = runCatching { uniffi.ducat_mobile.nodeWaitChange(WAIT_MS) }
                .getOrElse { Thread.sleep(1_000); false }
            if (!rang) continue
            // The whole answer to a ring, so one bad iteration is a
            // logged line and not the end of the lane: a lane that dies
            // leaves a phone that stops answering until it is restarted.
            runCatching {
                val moved = runCatching { uniffi.ducat_mobile.nodeChangedKeys() }
                    .getOrDefault(emptyList())
                NetworkRings.note(moved)
                val rangKeys = moved.toSet() // several subkeys, one record
                if (rangKeys.isEmpty()) return@runCatching
                val t0 = System.currentTimeMillis()
                val store = ContactStore(context)
                var got = 0
                val handled = mutableSetOf<String>()
                for (c in store.all()) {
                    if (c.theirOutbox in rangKeys) {
                        handled.add(c.theirOutbox)
                        got += runCatching { Mailbox.pollContact(context, c) }
                            .getOrDefault(0)
                    }
                }
                val cardKeys = store.issuedCards()
                    .filter { it.answeredBy == null }
                    .map { it.inboxKey }
                    .filter { it in rangKeys }
                if (cardKeys.isNotEmpty()) {
                    handled.addAll(cardKeys)
                    runCatching { Mailbox.collectClaims(context) }
                    runCatching { Listings.linkClaims(context) }
                }
                // A message that just landed may be a ringing offer, and no
                // screen is around to notice it for us.
                if (got > 0) runCatching { Calls.noticed(context) }
                val strangers = rangKeys - handled
                DucatLog.i(
                    TAG,
                    "lane: ${rangKeys.size} record(s) rang, $got message(s) in " +
                        "${System.currentTimeMillis() - t0} ms" +
                        if (strangers.isEmpty()) "" else
                            " — ${strangers.size} for the sweep (${strangers.first().take(12)}…)",
                )
            }.onFailure { DucatLog.w(TAG, "lane: ${it.javaClass.simpleName}: ${it.message}") }
        }
    }

    /** Watch health as last narrated — only changes are logged. */
    private var watchesUp = -1
    private var watchesDown = -1

    private companion object {
        /** One lane wait. Its length is invisible to latency — a ring
         *  interrupts it — it only sets how often an idle lane loops. */
        const val WAIT_MS = 10_000u

        /** Sweep pacing: a chunk between foreground passes, a heartbeat
         *  between pocket ones. The lane makes messages instant in both
         *  tiers; the heartbeat is the guarantee behind it. */
        const val CHUNK_MS = 10_000L
        const val HEARTBEAT_MS = 180_000L

        /**
         * How often a downed node is offered another start. Long enough that a
         * node genuinely unable to start is not thrashing its store, short
         * enough that somebody staring at "could not reach the network" gets
         * it back without knowing to force-quit.
         */
        const val REVIVE_EVERY_MS = 30_000L
    }
}
