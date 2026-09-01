package org.ducatproject.ducat

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/**
 * Live calls (§16.21), the client half: the thread carries the doors, the
 * routes carry the sound, and this object owns the one call a device can
 * be in.
 *
 * The platform half — a microphone and a speaker — is injected like
 * [DeviceLock]'s backend, because these shared sources compile on the desk
 * too and `android.media` does not. No backend means no media: the state
 * machine still answers and hangs up, which is what the desk's test roles
 * use.
 *
 * Media matches the spec: an 8-byte header (seq u32be ‖ ms u32be) then one
 * Opus packet — 16 kHz mono, 20 ms a frame, hard CBR so every frame leaves
 * the same size whether the speaker talks or holds their breath. The codec
 * itself lives in Rust with the node, shared by phone and desk.
 */
object Calls {
    /** 20 ms of PCM16 mono at 16 kHz. */
    const val FRAME_BYTES = 640
    const val FRAME_MS = 20L

    /**
     * An offer older than this is a missed call, not a ringing one. Long
     * for a telephone because the offer and its answer each ride the
     * mailbox: two cold DHT trips measured ~25–45 s each on a phone — a
     * 45 s window expired with the answer already in flight. The v2 fix is
     * answering back through the offer's own route (CALLS.md); until then
     * the phone rings the way a long-distance call once did.
     */
    const val RING_WINDOW_SECS = 90L

    /** The platform's ears and mouth; null on hosts without either. */
    interface Audio {
        /** Start capturing; deliver each 640-byte frame to [onFrame]. */
        fun start(onFrame: (ByteArray) -> Unit)

        /** Play one 640-byte frame. */
        fun play(frame: ByteArray)

        fun stop()

        /**
         * Ring: loud on the ringer stream for an incoming call, soft in the
         * earpiece as ringback while placing one. Hosts without a bell
         * inherit silence.
         */
        fun ring(context: Context, incoming: Boolean) {}

        fun quiet() {}
    }

    /**
     * The platform's way of opening the app for a ring nobody is looking
     * at, injected like [Audio] and for the same reason: these sources
     * compile on the desk, and notifications do not.
     */
    interface Shell {
        /** A call is ringing and no screen shows it: take the screen. */
        fun takeover(context: Context, from: String) {}

        /** The ring ended however it ended: give the screen back. */
        fun release(context: Context) {}
    }

    var shell: Shell? = null

    /**
     * One 3-second cycle of the British ring — 400 Hz + 450 Hz mixed, on
     * for 0.4 s, off 0.2, on 0.4, then two seconds of rest. PCM16LE mono
     * 16 kHz, ready to loop; 5 ms cosine ramps keep the edges clickless.
     */
    fun ukRing(amplitude: Double): ByteArray {
        val sr = 16_000
        val out = ByteArray(3 * sr * 2)
        val bursts = listOf(0.0 to 0.4, 0.6 to 1.0)
        for (i in 0 until 3 * sr) {
            val t = i.toDouble() / sr
            var env = 0.0
            for ((a, b) in bursts) {
                if (t >= a && t < b) {
                    val edge = kotlin.math.min(t - a, b - t)
                    env = if (edge >= 0.005) 1.0 else {
                        (1 - kotlin.math.cos(kotlin.math.PI * edge / 0.005)) / 2
                    }
                }
            }
            if (env == 0.0) continue
            val tone = kotlin.math.sin(2 * kotlin.math.PI * 400 * t) +
                kotlin.math.sin(2 * kotlin.math.PI * 450 * t)
            val v = (tone * 0.5 * env * amplitude * 32767).toInt()
            out[i * 2] = (v and 0xFF).toByte()
            out[i * 2 + 1] = ((v shr 8) and 0xFF).toByte()
        }
        return out
    }

    // §16.21 control frames: seq sentinel, type, call id, ANSWER's route.
    private const val CTRL_ANSWER = 1
    private const val CTRL_DECLINE = 2
    private const val CTRL_BYE = 3
    private const val CTRL_RENEW = 4

    private fun controlFrame(type: Int, id: ByteArray, route: ByteArray? = null): ByteArray {
        val out = ByteArray(8 + 1 + 8 + (route?.size ?: 0))
        for (i in 0..3) out[i] = 0xFF.toByte() // the sentinel; ms stays zero
        out[8] = type.toByte()
        id.copyInto(out, 9)
        route?.copyInto(out, 17)
        return out
    }

    private fun isControl(f: ByteArray) = f.size >= 17 &&
        f[0] == 0xFF.toByte() && f[1] == 0xFF.toByte() &&
        f[2] == 0xFF.toByte() && f[3] == 0xFF.toByte()

    sealed interface State {
        data object Idle : State
        data class Outgoing(val contactHex: String) : State
        /** Rang the window out, or they declined: the moment a telephone
         *  answering machine would pick up. The screen offers the thread's
         *  recorder; dismissing goes back to Idle. */
        data class NoAnswer(val contactHex: String) : State
        data class Incoming(val contactHex: String, val offerSeq: Long) : State
        data class Active(val contactHex: String, val sinceMs: Long) : State
    }

    var audio: Audio? = null

    var state by mutableStateOf<State>(State.Idle)
        private set

    /** Quality counters the screen renders — proof of life, not vanity. */
    var rxFrames by mutableStateOf(0)
        private set
    var txFrames by mutableStateOf(0)
        private set

    /** Bumped as each call begins; a delayed teardown from the last call
     *  must never close the routes of the next one. */
    private val epoch = java.util.concurrent.atomic.AtomicInteger()

    /** The application context, kept from the last entry point — the
     *  teardown paths need to quiet a notification with no caller handy. */
    @Volatile private var appCtx: Context? = null

    private var myCallId: ByteArray? = null
    private var theirRoute: ByteArray? = null
    private var pump: Thread? = null
    @Volatile private var running = false

    /** Ring somebody: allocate our door, send the offer, wait for theirs. */
    fun place(context: Context, c: Contact) {
        if (state != State.Idle) return
        val app = context.applicationContext
        appCtx = app
        val st = State.Outgoing(c.personaHex)
        state = st
        epoch.incrementAndGet()
        audio?.ring(app, incoming = false)
        Thread {
            runCatching {
                val mine = uniffi.ducat_mobile.nodeCallRoute()
                val id = ByteArray(8).also { java.security.SecureRandom().nextBytes(it) }
                myCallId = id
                DucatLog.i("Calls", "ringing with id=${id.toHexLower()}")
                Mailbox.send(
                    app, c, app.getString(R.string.call_body_ring),
                    kind = 14, callRoute = mine, callId = id,
                )
            }.onFailure {
                DucatLog.w("Calls", "place: ${it.message}")
                endInternal()
            }
        }.apply { isDaemon = true }.start()
        // A ringing call cannot wait for the background poll clock: the
        // answer is mailbox-borne and the window is finite. Poll THIS
        // contact only — the full sweep reads every contact's records and
        // once outlasted the whole ring; the thread dies with the state.
        Thread {
            while (state === st) {
                // The open door may answer before the mailbox does
                // (§16.21 control frames): the callee's ANSWER or DECLINE
                // arrives on the route the offer carried out.
                var f = uniffi.ducat_mobile.nodeCallRecv(0u)
                while (f != null && state === st) {
                    if (isControl(f) && myCallId != null &&
                        f.copyOfRange(9, 17).contentEquals(myCallId)
                    ) {
                        when (f[8].toInt()) {
                            CTRL_ANSWER -> if (f.size > 17) {
                                DucatLog.i("Calls", "answered at the door")
                                goActive(
                                    st.contactHex,
                                    f.copyOfRange(17, f.size),
                                    initiator = true,
                                )
                            }
                            CTRL_DECLINE -> {
                                DucatLog.i("Calls", "declined at the door")
                                endInternal(noAnswer = true)
                            }
                        }
                    }
                    if (state !== st) break
                    f = uniffi.ducat_mobile.nodeCallRecv(0u)
                }
                if (state !== st) break
                runCatching { Mailbox.pollContact(app, c) }
                noticed(app)
                Thread.sleep(2_000)
            }
        }.apply { isDaemon = true }.start()
        expireRing(st, RING_WINDOW_SECS)
    }

    /** Nobody picked up inside the window: stop being a ringing phone. */
    private fun expireRing(ringing: State, afterSecs: Long) {
        Thread {
            Thread.sleep(afterSecs * 1000)
            if (state === ringing) {
                DucatLog.i("Calls", "ring window over")
                endInternal(noAnswer = true)
            }
        }.apply { isDaemon = true }.start()
    }

    /** Answer a ringing offer: our door back, then sound both ways. */
    fun answer(context: Context, c: Contact, offer: StoredMessage) {
        if (state !is State.Incoming) return
        val app = context.applicationContext
        Thread {
            runCatching {
                epoch.incrementAndGet()
                val mine = uniffi.ducat_mobile.nodeCallRoute()
                val id = hexToBytes(offer.callId!!)
                myCallId = id
                // Through the door first — the caller connects in a route
                // trip; the sealed kind-15 below remains the record.
                val door = hexToBytes(offer.callRoute!!)
                repeat(2) {
                    runCatching {
                        uniffi.ducat_mobile.nodeCallSend(door, controlFrame(CTRL_ANSWER, id, mine))
                    }
                }
                Mailbox.send(
                    app, c, app.getString(R.string.call_body_answer),
                    kind = 15,
                    callRoute = mine,
                    callId = id,
                )
                goActive(c.personaHex, door, initiator = false)
            }.onFailure {
                DucatLog.w("Calls", "answer: ${it.message}")
                endInternal()
            }
        }.apply { isDaemon = true }.start()
    }

    /** Leave the answering-machine screen without leaving a message. */
    fun dismissNoAnswer() {
        if (state is State.NoAnswer) state = State.Idle
    }

    /** Decline is §16.13's Retract naming the offer — the till's own word. */
    fun decline(context: Context, c: Contact, offer: StoredMessage) {
        val app = context.applicationContext
        audio?.quiet()
        runCatching { shell?.release(app) }
        state = State.Idle
        Thread {
            runCatching {
                val door = offer.callRoute?.let { hexToBytes(it) }
                val id = offer.callId?.let { hexToBytes(it) }
                if (door != null && id != null) {
                    repeat(2) {
                        runCatching {
                            uniffi.ducat_mobile.nodeCallSend(door, controlFrame(CTRL_DECLINE, id))
                        }
                    }
                }
                Mailbox.send(
                    app, c, app.getString(R.string.call_body_decline),
                    kind = 5, reSeq = offer.seq, reOwn = false,
                )
            }.onFailure { DucatLog.w("Calls", "decline: ${it.message}") }
        }.apply { isDaemon = true }.start()
    }

    fun hangUp() = endInternal()

    /**
     * The store moved — see whether it moved for us. Called on every bump
     * from the app shell: a fresh inbound offer rings while we are idle; a
     * fresh answer quoting our id makes an outgoing call active.
     */
    fun noticed(context: Context) {
        appCtx = context.applicationContext
        val store = ContactStore(context)
        val now = System.currentTimeMillis() / 1000
        when (val s = state) {
            is State.Outgoing -> {
                val thread = store.thread(s.contactHex)
                val answer = thread.lastOrNull {
                    !it.outgoing && it.kind == 15 &&
                        it.callId != null && myCallId != null &&
                        it.callId == myCallId!!.toHexLower() &&
                        // A late answer to an expired ring must not open a
                        // call into a route its owner already tore down.
                        now - it.timestamp < RING_WINDOW_SECS * 2
                }
                if (answer?.callRoute != null) {
                    DucatLog.i("Calls", "answered: seq=${answer.seq} id=${answer.callId}")
                    goActive(s.contactHex, hexToBytes(answer.callRoute), initiator = true)
                    return
                }
                // Their Retract naming our offer is the §16.13 word for
                // "no" — stop ringing in their ear and ours.
                val offer = thread.lastOrNull {
                    it.outgoing && it.kind == 14 &&
                        myCallId != null && it.callId == myCallId!!.toHexLower()
                }
                if (offer != null && thread.any {
                        !it.outgoing && it.kind == 5 && it.reSeq == offer.seq && !it.reOwn
                    }
                ) {
                    DucatLog.i("Calls", "declined")
                    endInternal(noAnswer = true)
                }
            }
            State.Idle -> {
                for (c in store.all()) {
                    val offer = store.thread(c.personaHex).lastOrNull {
                        !it.outgoing && it.kind == 14 &&
                            now - it.timestamp < RING_WINDOW_SECS &&
                            it.callRoute != null && it.callId != null &&
                            // A ring already answered or declined is history.
                            store.thread(c.personaHex).none { r ->
                                r.outgoing && (
                                    (r.kind == 15 && r.callId == it.callId) ||
                                        (r.kind == 5 && r.reSeq == it.seq && !r.reOwn)
                                    )
                            }
                    } ?: continue
                    val st = State.Incoming(c.personaHex, offer.seq)
                    state = st
                    audio?.ring(context.applicationContext, incoming = true)
                    // Nobody may be looking at the app: the ring must open
                    // it — the full-screen ask on a dark phone, a banner on
                    // a lit one. The shell decides; the desk has none.
                    runCatching {
                        shell?.takeover(context.applicationContext, c.displayName())
                    }
                    // Ring only as long as the offer stays fresh.
                    expireRing(st, (RING_WINDOW_SECS - (now - offer.timestamp)).coerceIn(1, RING_WINDOW_SECS))
                    return
                }
            }
            else -> {}
        }
    }

    private fun goActive(contactHex: String, route: ByteArray, initiator: Boolean) {
        audio?.quiet()
        appCtx?.let { ctx -> runCatching { shell?.release(ctx) } }
        theirRoute = route
        rxFrames = 0
        txFrames = 0
        running = true
        // Anything queued before this call began is a previous life's sound.
        while (uniffi.ducat_mobile.nodeCallRecv(0u) != null) { /* drain */ }
        state = State.Active(contactHex, System.currentTimeMillis())
        // The mouth: capture, encode, stamp the header, out the door. The
        // side that ANSWERED holds its tongue until it first hears the
        // caller: its answer takes mailbox-seconds to arrive, and frames
        // sent into that gap pile up at the far end and play back late —
        // the caller transmits at once, because the answer in hand proves
        // the path is live.
        val seq = java.util.concurrent.atomic.AtomicInteger(0)
        val t0 = System.currentTimeMillis()
        audio?.start { frame ->
            if (!running) return@start
            if (!initiator && rxFrames == 0) return@start
            // The FIELD, not the parameter: a RENEW re-aims mid-call, and a
            // closure that captured the launch-time route would keep
            // whispering into the dead door for ever.
            val door = theirRoute ?: return@start
            val pkt = runCatching { uniffi.ducat_mobile.callEncode(frame) }
                .getOrNull() ?: return@start
            val n = seq.getAndIncrement()
            val ms = (System.currentTimeMillis() - t0).toInt()
            val out = ByteArray(8 + pkt.size)
            out[0] = (n ushr 24).toByte(); out[1] = (n ushr 16).toByte()
            out[2] = (n ushr 8).toByte(); out[3] = n.toByte()
            out[4] = (ms ushr 24).toByte(); out[5] = (ms ushr 16).toByte()
            out[6] = (ms ushr 8).toByte(); out[7] = ms.toByte()
            pkt.copyInto(out, 8)
            runCatching { uniffi.ducat_mobile.nodeCallSend(door, out) }
                .onSuccess { txFrames++ }
        }
        // The ear: drain the ring, drop the header, decode IN ORDER, play.
        // The decoder is stateful, so a frame from the past would smear the
        // present: stale arrivals are dropped, and a small gap is bridged
        // with Opus's own concealment — its guess at the lost 20 ms — which
        // also keeps the frames after the gap decoding clean. Ten seconds
        // of silence is the other side gone — the swarm's own watchdog
        // reflex, on a faster clock.
        pump = Thread {
            var lastHeard = System.currentTimeMillis()
            var lastSeq = -1L
            // The bad-draw watch (§16.21 RENEW): some routes lose most of a
            // direction while the reverse runs clean. When arrivals starve
            // against the 50 Hz cadence, allocate a fresh door and hand it
            // over on the direction that still works.
            var winStart = System.currentTimeMillis()
            var winCount = 0
            var lastRenew = 0L
            while (running) {
                val f = uniffi.ducat_mobile.nodeCallRecv(50u)
                val nowMs = System.currentTimeMillis()
                if (nowMs - winStart >= 6_000) {
                    if (rxFrames > 100 && winCount < 90 &&
                        nowMs - lastRenew > 15_000 && running
                    ) {
                        lastRenew = nowMs
                        val id = myCallId
                        val door = theirRoute
                        if (id != null && door != null) {
                            Thread {
                                runCatching {
                                    val fresh = uniffi.ducat_mobile.nodeCallRoute()
                                    DucatLog.i(
                                        "Calls",
                                        "starving (${winCount / 6}/s) — renewing our door",
                                    )
                                    repeat(3) {
                                        runCatching {
                                            uniffi.ducat_mobile.nodeCallSend(
                                                door, controlFrame(CTRL_RENEW, id, fresh),
                                            )
                                        }
                                    }
                                }
                            }.apply { isDaemon = true }.start()
                        }
                    }
                    winStart = nowMs
                    winCount = 0
                }
                if (f != null && isControl(f)) {
                    lastHeard = nowMs
                    val ctlId = f.copyOfRange(9, 17)
                    if (myCallId != null && ctlId.contentEquals(myCallId)) {
                        when (f[8].toInt()) {
                            CTRL_BYE -> {
                                DucatLog.i("Calls", "BYE — they hung up")
                                endInternal()
                            }
                            CTRL_RENEW -> if (f.size > 17) {
                                theirRoute = f.copyOfRange(17, f.size)
                                DucatLog.i("Calls", "re-aimed at their new door")
                            }
                        }
                    }
                } else if (f != null && f.size > 8) {
                    winCount++
                    lastHeard = System.currentTimeMillis()
                    val seq = ((f[0].toLong() and 0xFF) shl 24) or
                        ((f[1].toLong() and 0xFF) shl 16) or
                        ((f[2].toLong() and 0xFF) shl 8) or (f[3].toLong() and 0xFF)
                    if (seq <= lastSeq) continue // late echo of a concealed gap
                    val gap = seq - lastSeq - 1
                    if (lastSeq >= 0 && gap in 1..5) {
                        repeat(gap.toInt()) {
                            runCatching { uniffi.ducat_mobile.callConceal() }
                                .onSuccess { audio?.play(it) }
                        }
                    }
                    lastSeq = seq
                    rxFrames++
                    runCatching {
                        uniffi.ducat_mobile.callDecode(f.copyOfRange(8, f.size))
                    }.onSuccess { audio?.play(it) }
                } else if (System.currentTimeMillis() - lastHeard > 10_000) {
                    DucatLog.i("Calls", "silence — the far side hung up (rx=$rxFrames tx=$txFrames)")
                    endInternal()
                }
            }
        }.apply { isDaemon = true; name = "call-rx"; start() }
    }

    private fun endInternal(noAnswer: Boolean = false) {
        // Where the answering machine lives: an outgoing ring that never
        // became a call ends on the leave-a-message screen, not on a
        // silent jump back to wherever the phone was.
        val unanswered = (state as? State.Outgoing)?.contactHex
            ?.takeIf { noAnswer }
        val sayBye = running
        val route = theirRoute
        val id = myCallId
        running = false
        runCatching {
            DucatLog.i("Calls", "sends ok/failed: ${uniffi.ducat_mobile.nodeCallSendReport()}")
        }
        audio?.quiet()
        appCtx?.let { ctx -> runCatching { shell?.release(ctx) } }
        audio?.stop()
        myCallId = null
        theirRoute = null
        pump = null
        state = unanswered?.let { State.NoAnswer(it) } ?: State.Idle
        // The goodbye and the teardown leave together, off this thread —
        // hangUp arrives on a UI click. BYE goes out three times because
        // it is fire-and-forget; the far side's watchdog remains the
        // answer for a peer that crashed instead of saying it.
        val ep = epoch.get()
        Thread {
            if (sayBye && route != null && id != null) {
                repeat(3) {
                    runCatching {
                        uniffi.ducat_mobile.nodeCallSend(route, controlFrame(CTRL_BYE, id))
                    }
                }
                Thread.sleep(250) // let the queue drain before close purges it
            }
            if (epoch.get() == ep) {
                runCatching { uniffi.ducat_mobile.nodeCallClose() }
            }
        }.apply { isDaemon = true }.start()
    }

    private fun hexToBytes(hex: String): ByteArray =
        ByteArray(hex.length / 2) {
            ((Character.digit(hex[it * 2], 16) shl 4) +
                Character.digit(hex[it * 2 + 1], 16)).toByte()
        }

    private fun ByteArray.toHexLower(): String = joinToString("") { "%02x".format(it) }
}
