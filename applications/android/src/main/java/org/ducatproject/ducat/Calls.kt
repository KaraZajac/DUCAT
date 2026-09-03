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

    /**
     * How long the answering-machine screen stays up on its own — and so
     * how long after the ring an answer can still become the call: on a
     * phone the offer's trip alone can take most of the window, and an
     * answer given within a minute of the bell lands after the caller's
     * ninety seconds. The screen is the caller still standing there.
     */
    const val NO_ANSWER_LINGER_SECS = 45L

    /** The platform's ears and mouth; null on hosts without either. */
    interface Audio {
        /** Start capturing; deliver each 640-byte frame to [onFrame].
         *  False when the microphone would not open — a call with no
         *  mouth is one the far side hangs up on as silence. */
        fun start(onFrame: (ByteArray) -> Unit): Boolean

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

        /**
         * Sound is flowing with [from]. The host keeps the microphone alive
         * while the app is off the screen — Android 14 takes it from a
         * background app unless a service of the right type holds it — and
         * offers a hang-up from outside the app.
         */
        fun connected(context: Context, from: String) {}

        /**
         * This phone is ringing [to] — the moment to take what [connected]
         * needs. Android grants the microphone service type only to an app
         * on the screen, and the caller's is on the screen *now*; by the
         * time the far side answers it may be in a pocket, and a type
         * asked for then is refused, leaving the call to go silent the
         * next time the screen turns off.
         */
        fun calling(context: Context, to: String) {}

        /** The call is over, however it ended: undo [connected] and [calling]. */
        fun ended(context: Context) {}
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
         *  recorder; dismissing goes back to Idle. [why] is what the screen
         *  says instead of pretending somebody let it ring: the offer never
         *  left this phone, or the call was answered — here or there — and
         *  no sound ever arrived. */
        data class NoAnswer(val contactHex: String, val why: Why = Why.RANG_OUT) : State
        enum class Why {
            RANG_OUT,
            UNREACHED,
            /** Answered, then silence until the watchdog: the answerer of a
             *  stale ring whose caller had already given up, or a caller
             *  whose answerer's door never worked. Either way the two just
             *  missed each other, which is what the recorder is for. */
            NEVER_CONNECTED,
        }
        data class Incoming(val contactHex: String, val offerSeq: Long, val callId: String) : State
        /** Answer tapped: the bell is off and our door is on its way. The
         *  window's expiry and a second tap both find this, not Incoming.
         *  Carries the offer's coordinates because a hang-up from here is
         *  a decline, and the decline names the offer. */
        data class Answering(
            val contactHex: String,
            val offerSeq: Long,
            val callId: String,
            val door: String,
        ) : State
        data class Active(val contactHex: String, val sinceMs: Long) : State
    }

    /**
     * How far a caller's clock may lag ours before a fresh offer looks
     * expired: the timestamps in a thread are the *sender's*, and a phone
     * a minute behind used to ring for one second, or not at all. Offers
     * that old with an honest clock ring in vain for as long — the far side
     * has given up — and that is the cheaper mistake.
     */
    private const val CALL_SKEW_SECS = 60L

    /**
     * Rings already answered, declined or rung out here, by call id — not
     * by seq, which a fresh card restarts at 0. The outgoing kind-15 or
     * Retract that says so lands in the thread seconds after the tap —
     * after the sealing, before the DHT write — and every store bump in
     * between would find the offer unanswered and ring it again; an offer
     * that rang out is still inside the skew allowance when it stops.
     */
    private val dealtWith = java.util.Collections.synchronizedSet(HashSet<String>())

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

    // Both volatile, because both cross threads without a lock between
    // the write and the read. `theirRoute` is the one that matters: the
    // ear thread re-aims it on a RENEW and the mouth thread reads it on
    // every frame — the whole point of reading the field rather than a
    // captured route — and without this the mouth is allowed to go on
    // seeing the old door for as long as it likes.
    @Volatile private var myCallId: ByteArray? = null
    @Volatile private var theirRoute: ByteArray? = null
    private var pump: Thread? = null
    @Volatile private var running = false

    /** Ring somebody: allocate our door, send the offer, wait for theirs. */
    @Synchronized
    fun place(context: Context, c: Contact) {
        if (state != State.Idle) return
        val app = context.applicationContext
        appCtx = app
        // The id before the state: a hang-up during the seconds the offer
        // takes to seal needs it to withdraw the offer once it has landed.
        val id = ByteArray(8).also { java.security.SecureRandom().nextBytes(it) }
        myCallId = id
        val st = State.Outgoing(c.personaHex)
        state = st
        val ep = epoch.incrementAndGet()
        runCatching { audio?.ring(app, incoming = false) }
        runCatching { shell?.calling(app, c.displayName()) }
            .onFailure { DucatLog.w("Calls", "calling hook: ${it.message}") }
        Thread {
            runCatching {
                val mine = uniffi.ducat_mobile.nodeCallRoute()
                // Hung up while the door was being built: an offer sent
                // now rings their phone with a dead door, until the
                // withdrawal catches up with it. Nothing goes out.
                if (state !== st) {
                    if (epoch.get() == ep) runCatching { uniffi.ducat_mobile.nodeCallClose() }
                    return@runCatching
                }
                DucatLog.i("Calls", "ringing with id=${id.toHexLower()}")
                Mailbox.send(
                    app, c, app.getString(R.string.call_body_ring),
                    kind = 14, callRoute = mine, callId = id,
                )
            }.onFailure {
                DucatLog.w("Calls", "place: ${it.message}")
                // Not "no answer": nobody was asked. The screen says which.
                if (state === st) endInternal(noAnswer = true, unreached = true)
            }
        }.apply { isDaemon = true }.start()
        // A ringing call cannot wait for the background poll clock: the
        // answer is mailbox-borne and the window is finite. Poll THIS
        // contact only — the full sweep reads every contact's records and
        // once outlasted the whole ring; the thread dies with the state —
        // or with the answering-machine screen the ring ends on, which is
        // where a late answer is still worth something (see `noticed`).
        Thread {
            fun on() = state === st || (state as? State.NoAnswer)?.let {
                it.contactHex == st.contactHex && it.why == State.Why.RANG_OUT
            } == true
            while (on()) {
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
                                    st,
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
                if (!on()) break
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
                if (ringing is State.Incoming) stopRinging(ringing) else endInternal(noAnswer = true)
            }
        }.apply { isDaemon = true }.start()
    }

    /** Answer a ringing offer: our door back, then sound both ways. */
    @Synchronized
    fun answer(context: Context, c: Contact, offer: StoredMessage) {
        if (state !is State.Incoming) return
        // A ring is only ever started on an offer that has both (see
        // `noticed`); one without is nothing to answer.
        val doorHex = offer.callRoute ?: return
        val idHex = offer.callId ?: return
        val app = context.applicationContext
        appCtx = app
        // The bell stops at the tap, not when the door is built: our route
        // and the ANSWER take seconds, and a phone still ringing over them
        // invited a second tap — and let the window expire under the first.
        audio?.quiet()
        runCatching { shell?.release(app) }
        dealtWith.add(idHex)
        val st = State.Answering(c.personaHex, offer.seq, idHex, doorHex)
        state = st
        val ep = epoch.incrementAndGet()
        Thread {
            runCatching {
                val mine = uniffi.ducat_mobile.nodeCallRoute()
                // Hung up while the door was being built — seconds, tens
                // of them on a young node. An ANSWER sent now would open
                // the caller's mouth into a room nobody is in; the hang-up
                // was a decline (endInternal), and they have heard it.
                if (state !== st) {
                    if (epoch.get() == ep) runCatching { uniffi.ducat_mobile.nodeCallClose() }
                    return@runCatching
                }
                val id = hexToBytes(idHex)
                myCallId = id
                // Through the door first — the caller connects in a route
                // trip; the sealed kind-15 below remains the record.
                val door = hexToBytes(doorHex)
                repeat(2) {
                    runCatching {
                        uniffi.ducat_mobile.nodeCallSend(door, controlFrame(CTRL_ANSWER, id, mine))
                    }
                }
                // Sound before the record. The caller heard ANSWER at the
                // door and is already talking into ours; the kind-15 takes
                // mailbox-seconds, and an ear opened after it threw those
                // seconds away.
                if (!goActive(st, c.personaHex, door, initiator = false, patienceMs = patienceFor(offer))) {
                    if (epoch.get() == ep) runCatching { uniffi.ducat_mobile.nodeCallClose() }
                    return@runCatching
                }
                runCatching {
                    Mailbox.send(
                        app, c, app.getString(R.string.call_body_answer),
                        kind = 15,
                        callRoute = mine,
                        callId = id,
                    )
                }.onFailure { DucatLog.w("Calls", "answer record: ${it.message}") }
            }.onFailure {
                DucatLog.w("Calls", "answer: ${it.message}")
                if (state === st) endInternal(silent = true)
            }
        }.apply { isDaemon = true }.start()
    }

    /**
     * How long the answerer waits for the caller's first frame before
     * calling the silence a call that never was. Its ANSWER through the
     * door may have been lost, and the caller then finds our route in the
     * kind-15 — which it reads only while its ring, and then its
     * answering-machine screen, are still up: the window and the linger
     * from the offer's timestamp, by the caller's clock, which may lag
     * ours by the skew allowance. Past that, plus a poll and a route trip,
     * no first frame is coming; before it, giving up would abandon a
     * caller seconds from connecting. A flat window did both — the full
     * ninety seconds of "Connecting…" for a ring answered at the end of
     * its life, whose caller was long gone, and too little for a fresh
     * ring from a slow clock.
     */
    private fun patienceFor(offer: StoredMessage): Long {
        val theirs = RING_WINDOW_SECS + CALL_SKEW_SECS + NO_ANSWER_LINGER_SECS
        return ((offer.timestamp + theirs) * 1000 - System.currentTimeMillis() + 10_000)
            .coerceIn(10_000, theirs * 1000)
    }

    /** Leave the answering-machine screen without leaving a message. An
     *  answer from here on is the shade's news, not a call. */
    @Synchronized
    fun dismissNoAnswer() {
        if (state is State.NoAnswer) {
            myCallId = null
            state = State.Idle
        }
    }

    /** Decline is §16.13's Retract naming the offer — the till's own word. */
    @Synchronized
    fun decline(context: Context, c: Contact, offer: StoredMessage) {
        if (state !is State.Incoming) return
        val app = context.applicationContext
        appCtx = app
        audio?.quiet()
        runCatching { shell?.release(app) }
        offer.callId?.let { dealtWith.add(it) }
        state = State.Idle
        Thread {
            refuse(c.personaHex, offer.seq, offer.callId, offer.callRoute)
        }.apply { isDaemon = true }.start()
    }

    /** The "no" itself: at the door, where the caller's ring hears it in a
     *  route trip, and by mailbox, which is the record. Off the UI thread. */
    private fun refuse(contactHex: String, offerSeq: Long, idHex: String?, doorHex: String?) {
        val app = appCtx ?: return
        runCatching {
            val door = doorHex?.let { hexToBytes(it) }
            val id = idHex?.let { hexToBytes(it) }
            if (door != null && id != null) {
                repeat(2) {
                    runCatching {
                        uniffi.ducat_mobile.nodeCallSend(door, controlFrame(CTRL_DECLINE, id))
                    }
                }
            }
            val c = ContactStore(app).all().firstOrNull { it.personaHex == contactHex }
                ?: return@runCatching
            Mailbox.send(
                app, c, app.getString(R.string.call_body_decline),
                kind = 5, reSeq = offerSeq, reOwn = false,
            )
        }.onFailure { DucatLog.w("Calls", "decline: ${it.message}") }
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
                        now - it.timestamp < RING_WINDOW_SECS * 2 + CALL_SKEW_SECS
                }
                if (answer?.callRoute != null) {
                    DucatLog.i("Calls", "answered: seq=${answer.seq} id=${answer.callId}")
                    goActive(s, s.contactHex, hexToBytes(answer.callRoute), initiator = true)
                    return
                }
                // Their Retract naming our offer is the §16.13 word for
                // "no" — stop ringing in their ear and ours.
                val offer = thread.lastOrNull {
                    it.outgoing && it.kind == 14 &&
                        myCallId != null && it.callId == myCallId!!.toHexLower()
                }
                // Read as `referent` reads a reference — a seq is per card,
                // and a "no" to an old card's seq-3 must not end a fresh
                // ring at seq 3.
                if (offer != null && thread.any {
                        !it.outgoing && it.kind == 5 && !it.reOwn &&
                            thread.referent(it) === offer
                    }
                ) {
                    DucatLog.i("Calls", "declined")
                    endInternal(noAnswer = true)
                }
            }
            is State.NoAnswer -> {
                // The answer that lands after the ring ran out, while the
                // answering-machine screen is still up: they picked up, we
                // are still here, and the only thing missing is our door,
                // which went with the ring. A fresh one goes over as the
                // RENEW their pump already understands, and the call goes
                // up as if the answer had been on time. Once the screen is
                // dismissed the same answer is news for the shade instead.
                val id = myCallId
                if (s.why != State.Why.RANG_OUT || id == null) return
                val answer = store.thread(s.contactHex).lastOrNull {
                    !it.outgoing && it.kind == 15 &&
                        it.callId == id.toHexLower() &&
                        now - it.timestamp < RING_WINDOW_SECS * 2 + CALL_SKEW_SECS
                }
                if (answer?.callRoute != null) {
                    reopen(s, hexToBytes(answer.callRoute), id)
                }
            }
            is State.Incoming -> {
                // The caller withdrew the offer — hung up before we picked
                // up. Their Retract names their own message, by the seq
                // `referent` resolves; the offer itself is known by call id.
                val thread = store.thread(s.contactHex)
                val offer = thread.lastOrNull {
                    !it.outgoing && it.kind == 14 && it.callId == s.callId
                }
                if (offer != null && thread.any {
                        !it.outgoing && it.kind == 5 && it.reOwn &&
                            thread.referent(it) === offer
                    }
                ) {
                    DucatLog.i("Calls", "caller hung up before the answer")
                    stopRinging(s)
                }
            }
            State.Idle -> {
                for (c in store.all()) {
                    val thread = store.thread(c.personaHex)
                    val offer = thread.lastOrNull {
                        !it.outgoing && it.kind == 14 &&
                            // Fresh means "not yet due", which is where
                            // Elapsed's future rule earns its place: this
                            // stamp is the *caller's*, CALL_SKEW_SECS
                            // forgives one running behind, and nothing
                            // bounded one running ahead — an offer stamped
                            // in the future stayed fresh for ever and rang
                            // this phone on every start for a call nobody
                            // was on. A stamp this phone cannot vouch for
                            // is not a ringing telephone.
                            !Elapsed.dueSecs(
                                now, it.timestamp, RING_WINDOW_SECS + CALL_SKEW_SECS,
                            ) &&
                            it.callRoute != null && it.callId != null &&
                            it.callId !in dealtWith &&
                            // A ring already answered, declined or withdrawn
                            // is history. A Retract names the offer in its
                            // sender's numbering — ours with reOwn false,
                            // theirs with reOwn true — which is what
                            // `referent` reads, positionally, since a seq
                            // is per card.
                            thread.none { r ->
                                (r.outgoing && r.kind == 15 && r.callId == it.callId) ||
                                    (r.kind == 5 && thread.referent(r) === it)
                            }
                    } ?: continue
                    if (startRinging(context, c, offer, now)) return
                }
            }
            else -> {}
        }
    }

    /** One bell: the shell and the poller both notice, and the second to
     *  arrive must find the phone already ringing rather than ring twice. */
    @Synchronized
    private fun startRinging(context: Context, c: Contact, offer: StoredMessage, now: Long): Boolean {
        if (state != State.Idle) return false
        val st = State.Incoming(c.personaHex, offer.seq, offer.callId!!)
        state = st
        runCatching { audio?.ring(context.applicationContext, incoming = true) }
        // Nobody may be looking at the app: the ring must open it — the
        // full-screen ask on a dark phone, a banner on a lit one. The shell
        // decides; the desk has none.
        runCatching {
            shell?.takeover(context.applicationContext, c.displayName())
        }
        // Ring only as long as the offer stays fresh — by the caller's
        // clock, given the benefit of the skew.
        expireRing(
            st,
            (RING_WINDOW_SECS + CALL_SKEW_SECS - (now - offer.timestamp))
                .coerceIn(1, RING_WINDOW_SECS),
        )
        return true
    }

    /** A ring that stops without an answer from this side: the caller
     *  withdrew, or nobody picked up in time. Nothing to send. */
    @Synchronized
    private fun stopRinging(ringing: State.Incoming) {
        if (state !== ringing) return
        audio?.quiet()
        appCtx?.let { ctx -> runCatching { shell?.release(ctx) } }
        dealtWith.add(ringing.callId)
        state = State.Idle
    }

    /** The answering-machine screen a late answer is already being built
     *  for: every store bump finds the same answer, and one door is enough. */
    private var reopening: State? = null

    /**
     * A late answer's door. Ours was released with the ring, so build
     * another and hand it over before the first word — the answerer's
     * mouth aims wherever the last RENEW said, and holds its tongue until
     * it hears us, so the RENEW ahead of our first frame is in time. Off
     * the caller's thread: a route takes seconds, and on a node that only
     * just attached, tens of them; a screen dismissed meanwhile gets its
     * door taken down again — unless a newer call has begun, whose own
     * teardown then owns every route.
     */
    @Synchronized
    private fun reopen(from: State.NoAnswer, route: ByteArray, id: ByteArray) {
        if (state !== from || reopening === from) return
        reopening = from
        val ep = epoch.get()
        Thread {
            runCatching {
                val fresh = uniffi.ducat_mobile.nodeCallRoute()
                if (state !== from) {
                    if (epoch.get() == ep) runCatching { uniffi.ducat_mobile.nodeCallClose() }
                    return@runCatching
                }
                DucatLog.i("Calls", "answered late (id=${id.toHexLower()}) — reopening our door")
                repeat(3) {
                    runCatching {
                        uniffi.ducat_mobile.nodeCallSend(route, controlFrame(CTRL_RENEW, id, fresh))
                    }
                }
                goActive(from, from.contactHex, route, initiator = true)
            }.onFailure {
                DucatLog.w("Calls", "reopen: ${it.message}")
                // Let the next bump try again. This latch is "a door is
                // already being built for this screen", and a route that
                // failed to build — a node still finding its feet, which
                // is when this happens — left it latched for the life of
                // the answering-machine screen: the answer sat in the
                // thread, every bump saw it, and nothing was ever built.
                synchronized(this) { if (reopening === from) reopening = null }
            }
        }.apply { isDaemon = true }.start()
    }

    /**
     * Sound both ways, from [from] — the ring or the answer this call grew
     * out of. False when that moment has passed: the ring was hung up on
     * while the door was being built, or the door's ANSWER and the mailbox's
     * kind-15 both arrived and the second found the call already up — or
     * when there is no microphone to speak into. [patienceMs] is how long
     * the first frame may take before the silence is read as nobody there:
     * a route trip for the caller, who holds the answer in hand; for the
     * answerer, see [patienceFor].
     */
    @Synchronized
    private fun goActive(
        from: State,
        contactHex: String,
        route: ByteArray,
        initiator: Boolean,
        patienceMs: Long = 10_000L,
    ): Boolean {
        if (state !== from) return false
        audio?.quiet()
        appCtx?.let { ctx -> runCatching { shell?.release(ctx) } }
        theirRoute = route
        rxFrames = 0
        txFrames = 0
        running = true
        // The caller's queue holds a previous life's sound, if anything;
        // the answerer's holds the caller's first words, spoken the moment
        // ANSWER reached them — while this side was still on its way here.
        if (initiator) {
            while (uniffi.ducat_mobile.nodeCallRecv(0u) != null) { /* drain */ }
        }
        state = State.Active(contactHex, System.currentTimeMillis())
        appCtx?.let { ctx ->
            runCatching {
                val name = ContactStore(ctx).all()
                    .firstOrNull { it.personaHex == contactHex }?.displayName() ?: ""
                shell?.connected(ctx, name)
            }
        }
        // The mouth: capture, encode, stamp the header, out the door. The
        // side that ANSWERED holds its tongue until it first hears the
        // caller: its answer takes mailbox-seconds to arrive, and frames
        // sent into that gap pile up at the far end and play back late —
        // the caller transmits at once, because the answer in hand proves
        // the path is live.
        val seq = java.util.concurrent.atomic.AtomicInteger(0)
        val t0 = System.currentTimeMillis()
        val mouth = audio?.start { frame ->
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
        if (mouth == false) {
            // Another app has the microphone, or it broke: a call we cannot
            // speak into is one the far side ends as silence in ten seconds
            // — better ended here, now, with the reason in the log.
            DucatLog.w("Calls", "no microphone — ending the call")
            endInternal()
            return false
        }
        // The ear: drain the ring, drop the header, decode IN ORDER, play.
        // The decoder is stateful, so a frame from the past would smear the
        // present: stale arrivals are dropped, and a small gap is bridged
        // with Opus's own concealment — its guess at the lost 20 ms — which
        // also keeps the frames after the gap decoding clean. Ten seconds
        // of silence is the other side gone — the swarm's own watchdog
        // reflex, on a faster clock. Before the first frame the answering
        // side waits longer instead — [patienceMs], the rest of the
        // caller's own window: its ANSWER at the door may have been lost,
        // and the caller then finds our route in the kind-15, which is
        // mailbox-seconds behind.
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
                } else if (System.currentTimeMillis() - lastHeard > (if (rxFrames == 0) patienceMs else 10_000L)) {
                    DucatLog.i("Calls", "silence — the far side hung up (rx=$rxFrames tx=$txFrames)")
                    // Not one frame: the screen says the call never was,
                    // rather than a timer that ran and then a wallet.
                    endInternal(silent = rxFrames == 0)
                }
            }
        }.apply { isDaemon = true; name = "call-rx"; start() }
        return true
    }

    /**
     * [silent]: answered — here or there — and not one frame arrived.
     * The same screen as a ring nobody picked up, worded for what it was:
     * the person is one tap from the recorder either way.
     */
    @Synchronized
    private fun endInternal(noAnswer: Boolean = false, unreached: Boolean = false, silent: Boolean = false) {
        // Where the answering machine lives: an outgoing ring that never
        // became a call ends on the leave-a-message screen, not on a
        // silent jump back to wherever the phone was.
        val ringing = state as? State.Outgoing
        val unanswered = ringing?.contactHex?.takeIf { noAnswer }
        val unheard = when (val s = state) {
            is State.Active -> s.contactHex
            is State.Answering -> s.contactHex
            else -> null
        }?.takeIf { silent }
        // Hung up on our own ring: the offer is still ringing in their
        // pocket, for the rest of the window, unless we take it back.
        val withdraw = ringing?.contactHex?.takeIf { !noAnswer }
        // Hung up on our own answer, while the door was still being built:
        // their phone is still ringing, and this is a "no" — the same one
        // the decline button says, which stops it. Not when the watchdog
        // ended it: that call was answered, and nothing arrived.
        val refusing = (state as? State.Answering)?.takeIf { !silent }
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
        appCtx?.let { ctx -> runCatching { shell?.ended(ctx) } }
        // The id outlives a ring that ran out: it is how a late answer is
        // known for ours while the answering-machine screen is up.
        myCallId = if (unanswered != null && !unreached) myCallId else null
        theirRoute = null
        pump = null
        reopening = null
        state = when {
            unanswered != null ->
                State.NoAnswer(unanswered, if (unreached) State.Why.UNREACHED else State.Why.RANG_OUT)
            unheard != null -> State.NoAnswer(unheard, State.Why.NEVER_CONNECTED)
            else -> State.Idle
        }
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
            if (refusing != null) {
                refuse(refusing.contactHex, refusing.offerSeq, refusing.callId, refusing.door)
                Thread.sleep(250)
            }
            if (withdraw != null && id != null) withdrawOffer(withdraw, id.toHexLower())
            if (epoch.get() == ep) {
                runCatching { uniffi.ducat_mobile.nodeCallClose() }
            }
        }.apply { isDaemon = true }.start()
    }

    /**
     * Take back a ring we hung up on: §16.13's Retract naming our own
     * offer, which stops the far phone the way their decline stops ours.
     * The offer's row lands as it is sealed, before its DHT write returns,
     * so a hang-up inside those seconds waits for the row; the send then
     * queues behind the placing thread on the contact's lock.
     */
    private fun withdrawOffer(contactHex: String, idHex: String) {
        val app = appCtx ?: return
        runCatching {
            val store = ContactStore(app)
            fun row() = store.thread(contactHex).lastOrNull {
                it.outgoing && it.kind == 14 && it.callId == idHex
            }
            // Backing off rather than once a second, which was wrong at both
            // ends. `row()` re-reads and re-parses the whole conversation, so
            // the wait for a row that never comes — the offer failed to seal
            // at all — cost ninety of those. And a full second is a long time
            // to wait for a race measured in milliseconds: the row is written
            // locally, before the DHT write it precedes, so the common case
            // is the very first retry.
            val deadline = System.currentTimeMillis() + RING_WINDOW_SECS * 1000
            var offer = row()
            var wait = 100L
            while (offer == null) {
                val left = deadline - System.currentTimeMillis()
                if (left <= 0) break
                Thread.sleep(wait.coerceAtMost(left))
                wait = (wait * 2).coerceAtMost(5_000L)
                offer = row()
            }
            val c = store.all().firstOrNull { it.personaHex == contactHex }
            if (offer == null || c == null) return@runCatching
            Mailbox.send(
                app, c, app.getString(R.string.call_body_cancel),
                kind = 5, reSeq = offer.seq, reOwn = true,
            )
            DucatLog.i("Calls", "withdrew the offer")
        }.onFailure { DucatLog.w("Calls", "withdraw: ${it.message}") }
    }

    private fun hexToBytes(hex: String): ByteArray =
        ByteArray(hex.length / 2) {
            ((Character.digit(hex[it * 2], 16) shl 4) +
                Character.digit(hex[it * 2 + 1], 16)).toByte()
        }

    private fun ByteArray.toHexLower(): String = joinToString("") { "%02x".format(it) }
}
