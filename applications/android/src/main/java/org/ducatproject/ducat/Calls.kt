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

    /** An offer older than this is a missed call, not a ringing one. */
    const val RING_WINDOW_SECS = 45L

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

    sealed interface State {
        data object Idle : State
        data class Outgoing(val contactHex: String) : State
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

    private var myCallId: ByteArray? = null
    private var theirRoute: ByteArray? = null
    private var pump: Thread? = null
    @Volatile private var running = false

    /** Ring somebody: allocate our door, send the offer, wait for theirs. */
    fun place(context: Context, c: Contact) {
        if (state != State.Idle) return
        val app = context.applicationContext
        val st = State.Outgoing(c.personaHex)
        state = st
        audio?.ring(app, incoming = false)
        Thread {
            runCatching {
                val mine = uniffi.ducat_mobile.nodeCallRoute()
                val id = ByteArray(8).also { java.security.SecureRandom().nextBytes(it) }
                myCallId = id
                Mailbox.send(
                    app, c, app.getString(R.string.call_body_ring),
                    kind = 14, callRoute = mine, callId = id,
                )
            }.onFailure {
                DucatLog.w("Calls", "place: ${it.message}")
                endInternal()
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
                endInternal()
            }
        }.apply { isDaemon = true }.start()
    }

    /** Answer a ringing offer: our door back, then sound both ways. */
    fun answer(context: Context, c: Contact, offer: StoredMessage) {
        if (state !is State.Incoming) return
        val app = context.applicationContext
        Thread {
            runCatching {
                val mine = uniffi.ducat_mobile.nodeCallRoute()
                Mailbox.send(
                    app, c, app.getString(R.string.call_body_answer),
                    kind = 15,
                    callRoute = mine,
                    callId = hexToBytes(offer.callId!!),
                )
                goActive(c.personaHex, hexToBytes(offer.callRoute!!), initiator = false)
            }.onFailure {
                DucatLog.w("Calls", "answer: ${it.message}")
                endInternal()
            }
        }.apply { isDaemon = true }.start()
    }

    /** Decline is §16.13's Retract naming the offer — the till's own word. */
    fun decline(context: Context, c: Contact, offer: StoredMessage) {
        val app = context.applicationContext
        audio?.quiet()
        state = State.Idle
        Thread {
            runCatching {
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
        val store = ContactStore(context)
        val now = System.currentTimeMillis() / 1000
        when (val s = state) {
            is State.Outgoing -> {
                val answer = store.thread(s.contactHex).lastOrNull {
                    !it.outgoing && it.kind == 15 &&
                        it.callId != null && myCallId != null &&
                        it.callId == myCallId!!.toHexLower()
                }
                if (answer?.callRoute != null) {
                    goActive(s.contactHex, hexToBytes(answer.callRoute), initiator = true)
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
            runCatching { uniffi.ducat_mobile.nodeCallSend(route, out) }
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
            while (running) {
                val f = uniffi.ducat_mobile.nodeCallRecv(50u)
                if (f != null && f.size > 8) {
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
                    DucatLog.i("Calls", "silence — the far side hung up")
                    endInternal()
                }
            }
        }.apply { isDaemon = true; name = "call-rx"; start() }
    }

    private fun endInternal() {
        running = false
        audio?.quiet()
        audio?.stop()
        runCatching { uniffi.ducat_mobile.nodeCallClose() }
        myCallId = null
        theirRoute = null
        pump = null
        state = State.Idle
    }

    private fun hexToBytes(hex: String): ByteArray =
        ByteArray(hex.length / 2) {
            ((Character.digit(hex[it * 2], 16) shl 4) +
                Character.digit(hex[it * 2 + 1], 16)).toByte()
        }

    private fun ByteArray.toHexLower(): String = joinToString("") { "%02x".format(it) }
}
