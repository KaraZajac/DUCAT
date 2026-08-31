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
 * v0 media matches the spec's provisional format: an 8-byte header (seq
 * u32be ‖ ms u32be) then 20 ms of PCM16 mono 16 kHz — 640 payload bytes,
 * 50 frames a second, a tenth of what the routes were measured to carry.
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
        state = State.Outgoing(c.personaHex)
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
                goActive(c.personaHex, hexToBytes(offer.callRoute!!))
            }.onFailure {
                DucatLog.w("Calls", "answer: ${it.message}")
                endInternal()
            }
        }.apply { isDaemon = true }.start()
    }

    /** Decline is §16.13's Retract naming the offer — the till's own word. */
    fun decline(context: Context, c: Contact, offer: StoredMessage) {
        val app = context.applicationContext
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
                    goActive(s.contactHex, hexToBytes(answer.callRoute))
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
                    state = State.Incoming(c.personaHex, offer.seq)
                    return
                }
            }
            else -> {}
        }
    }

    private fun goActive(contactHex: String, route: ByteArray) {
        theirRoute = route
        rxFrames = 0
        txFrames = 0
        running = true
        state = State.Active(contactHex, System.currentTimeMillis())
        // The mouth: capture frames, stamp the header, out the door.
        val seq = java.util.concurrent.atomic.AtomicInteger(0)
        val t0 = System.currentTimeMillis()
        audio?.start { frame ->
            if (!running) return@start
            val n = seq.getAndIncrement()
            val ms = (System.currentTimeMillis() - t0).toInt()
            val out = ByteArray(8 + frame.size)
            out[0] = (n ushr 24).toByte(); out[1] = (n ushr 16).toByte()
            out[2] = (n ushr 8).toByte(); out[3] = n.toByte()
            out[4] = (ms ushr 24).toByte(); out[5] = (ms ushr 16).toByte()
            out[6] = (ms ushr 8).toByte(); out[7] = ms.toByte()
            frame.copyInto(out, 8)
            runCatching { uniffi.ducat_mobile.nodeCallSend(route, out) }
                .onSuccess { txFrames++ }
        }
        // The ear: drain the ring, drop the header, play. Ten seconds of
        // silence is the other side gone — the swarm's own watchdog reflex,
        // on a faster clock.
        pump = Thread {
            var lastHeard = System.currentTimeMillis()
            while (running) {
                val f = uniffi.ducat_mobile.nodeCallRecv(50u)
                if (f != null && f.size > 8) {
                    lastHeard = System.currentTimeMillis()
                    rxFrames++
                    audio?.play(f.copyOfRange(8, f.size))
                } else if (System.currentTimeMillis() - lastHeard > 10_000) {
                    DucatLog.i("Calls", "silence — the far side hung up")
                    endInternal()
                }
            }
        }.apply { isDaemon = true; name = "call-rx"; start() }
    }

    private fun endInternal() {
        running = false
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
