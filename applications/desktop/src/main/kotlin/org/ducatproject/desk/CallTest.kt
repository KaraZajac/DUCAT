package org.ducatproject.desk

import java.io.File
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.sin
import kotlin.math.sqrt
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.NameStore
import uniffi.ducat_mobile.callDecode
import uniffi.ducat_mobile.callEncode
import uniffi.ducat_mobile.nodeCallClose
import uniffi.ducat_mobile.nodeCallRecv
import uniffi.ducat_mobile.nodeCallRoute
import uniffi.ducat_mobile.nodeCallSend
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus

/**
 * A live call between two desks, §16.21 whole: the offer and answer ride
 * the sealed thread (kinds 14–15, the door and its name), and fifteen
 * seconds of full-duplex audio ride app messages on the exchanged routes.
 *
 * Media: 8-byte header (seq u32be ‖ ms u32be) + one Opus packet — 16 kHz
 * mono, 20 ms, hard CBR (60 bytes, every frame, by design). The tone is a
 * deterministic 440 Hz sine; Opus is lossy, so the receiver verifies each
 * decoded frame is still *that tone* — frequency by zero crossings, level
 * by RMS — rather than byte equality.
 *
 *   DUCAT_CALL_ROLE=callee DUCAT_CALL_STATE=<dir>
 *   DUCAT_CALL_ROLE=caller DUCAT_CALL_STATE=<dir> DUCAT_CALL_CARD=<uri>
 *   DUCAT_CALL_ROLE=answerphone DUCAT_CALL_STATE=<dir>   # answers any ring
 *   DUCAT_CALL_ROLE=callback DUCAT_CALL_STATE=<dir>      # rings its contact
 *
 * Markers: CALL_CARD, CALL_RINGING, CALL_ANSWERED, CALL_STATS, CALL_OK;
 * the answerphone prints ANSWERPHONE_STATS, the callback CALLBACK_STATS /
 * CALLBACK_DECLINED / CALLBACK_UNANSWERED.
 */

private const val FRAMES = 750
private const val FRAME_MS = 20L
private const val SAMPLES = 320 // 20 ms at 16 kHz

private fun up(dir: File): DeskContext {
    val context = DeskContext(dir)
    nodeStart("${dir.absolutePath}/veilid", true)
    val deadline = System.currentTimeMillis() + 240_000
    while (System.currentTimeMillis() < deadline && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "CALL_FAIL node never became ready" }
    System.err.println("node ready")
    return context
}

/** This seq's 20 ms of the 440 Hz tone, raw PCM16LE. */
private fun tonePcm(seq: Int): ByteArray {
    val out = ByteArray(SAMPLES * 2)
    for (i in 0 until SAMPLES) {
        val t = (seq * SAMPLES + i).toDouble() / 16_000.0
        val v = (sin(2 * PI * 440.0 * t) * 12_000).toInt()
        out[i * 2] = (v and 0xFF).toByte()
        out[i * 2 + 1] = ((v shr 8) and 0xFF).toByte()
    }
    return out
}

/** Header on, packet behind: one wire frame. */
private fun framed(seq: Int, pkt: ByteArray): ByteArray {
    val out = ByteArray(8 + pkt.size)
    out[0] = (seq ushr 24).toByte(); out[1] = (seq ushr 16).toByte()
    out[2] = (seq ushr 8).toByte(); out[3] = seq.toByte()
    val ms = seq * FRAME_MS
    out[4] = (ms ushr 24).toByte(); out[5] = (ms ushr 16).toByte()
    out[6] = (ms ushr 8).toByte(); out[7] = ms.toByte()
    pkt.copyInto(out, 8)
    return out
}

/** Still the tone? ~17.6 sign changes per 20 ms of 440 Hz, sane RMS. */
private fun toneOk(pcm: ByteArray): Boolean {
    if (pcm.size != SAMPLES * 2) return false
    var zc = 0
    var acc = 0.0
    var prev = 0
    for (i in 0 until SAMPLES) {
        val v = (pcm[i * 2 + 1].toInt() shl 8) or (pcm[i * 2].toInt() and 0xFF)
        acc += v.toDouble() * v
        if (i > 0 && (v >= 0) != (prev >= 0)) zc++
        prev = v
    }
    val rms = sqrt(acc / SAMPLES)
    return zc in 15..21 && rms in 5_000.0..11_000.0
}

private fun media(theirRoute: ByteArray, initiator: Boolean): Boolean {
    var sent = 0
    var wireBytes = 0L
    var encNanos = 0L
    val recvSeqs = HashSet<Int>()
    var badFrames = 0
    var lastSeq = -1
    val arrivals = java.util.Collections.synchronizedList(ArrayList<Long>(FRAMES))

    // The receiver drains on its own thread — full duplex, like a call.
    // Its window is anchored on the FIRST arrival, not on our own start:
    // the two ends enter media mailbox-seconds apart.
    val rx = Thread {
        val cap = System.currentTimeMillis() + 120_000
        while (System.currentTimeMillis() < cap) {
            if (recvSeqs.size >= FRAMES) break
            synchronized(arrivals) {
                if (arrivals.isNotEmpty() &&
                    System.currentTimeMillis() > arrivals[0] + FRAMES * FRAME_MS + 8_000
                ) {
                    return@Thread
                }
            }
            val f = nodeCallRecv(50u) ?: continue
            arrivals.add(System.currentTimeMillis())
            if (f.size <= 8) { badFrames++; continue }
            val seq = ((f[0].toInt() and 0xFF) shl 24) or ((f[1].toInt() and 0xFF) shl 16) or
                ((f[2].toInt() and 0xFF) shl 8) or (f[3].toInt() and 0xFF)
            // The engine's own discipline: in order, conceal small gaps so
            // the decoder stays continuous, drop what arrives too late.
            if (seq <= lastSeq && lastSeq >= 0) { recvSeqs.add(seq); continue }
            val gap = if (lastSeq < 0) 0 else seq - lastSeq - 1
            if (gap in 1..5) repeat(gap) { runCatching { uniffi.ducat_mobile.callConceal() } }
            lastSeq = seq
            val pcm = runCatching { callDecode(f.copyOfRange(8, f.size)) }.getOrNull()
            // A frame right after a gap decodes from a concealed guess —
            // judge the tone only on frames with settled history.
            val judged = seq >= 3 && gap == 0
            if (pcm == null || (judged && !toneOk(pcm))) badFrames++
            recvSeqs.add(seq)
        }
    }.apply { start() }

    // The answering side holds its tongue until it first hears the caller —
    // its answer travels by mailbox, and frames sent into that gap pile up
    // at a far end that has not started listening.
    if (!initiator) {
        val waitUntil = System.currentTimeMillis() + 60_000
        while (arrivals.isEmpty() && System.currentTimeMillis() < waitUntil) Thread.sleep(20)
    }

    val t0 = System.currentTimeMillis()
    for (seq in 0 until FRAMES) {
        val due = t0 + seq * FRAME_MS
        val wait = due - System.currentTimeMillis()
        if (wait > 0) Thread.sleep(wait)
        val e0 = System.nanoTime()
        val pkt = runCatching { callEncode(tonePcm(seq)) }.getOrNull()
        encNanos += System.nanoTime() - e0
        if (pkt == null) continue
        val frame = framed(seq, pkt)
        runCatching { nodeCallSend(theirRoute, frame) }
            .onSuccess { sent++; wireBytes += frame.size }
            .onFailure { if (seq % 100 == 0) System.err.println("send $seq: ${it.message}") }
    }
    rx.join()

    val loss = FRAMES - recvSeqs.size
    // Where did the losses live — a cut head, a cut tail, or spread thin?
    if (recvSeqs.isNotEmpty()) {
        val bands = IntArray(5)
        for (s in recvSeqs) bands[(s * 5 / FRAMES).coerceIn(0, 4)]++
        System.err.println(
            "rx shape: minSeq=${recvSeqs.min()} maxSeq=${recvSeqs.max()} " +
                "fifths=${bands.joinToString("/")} " +
                "span=${arrivals.last() - arrivals.first()}ms",
        )
    }
    // RFC 3550-flavoured jitter on the arrival cadence.
    var jitter = 0.0
    for (i in 1 until arrivals.size) {
        jitter += abs((arrivals[i] - arrivals[i - 1]) - FRAME_MS).toDouble()
    }
    if (arrivals.size > 1) jitter /= (arrivals.size - 1)
    println(
        "CALL_STATS sent=$sent recv=${recvSeqs.size} loss=$loss " +
            "badframes=$badFrames jitter=${"%.1f".format(jitter)}ms " +
            "wire=${wireBytes / maxOf(sent, 1)}B/frame " +
            "enc=${encNanos / 1000 / maxOf(sent, 1)}µs/frame",
    )
    // Route quality varies per allocation — a lucky pair loses nothing, an
    // unlucky hop drops a tenth. Voice with concealment stays usable to
    // ~15% loss; past that, or if arrived frames stop decoding to the
    // tone, something is actually broken.
    return loss <= FRAMES * 15 / 100 && badFrames <= maxOf(2, recvSeqs.size / 50)
}

fun main() {
    val role = System.getenv("DUCAT_CALL_ROLE") ?: error("CALL_FAIL set DUCAT_CALL_ROLE")
    val dir = File(System.getenv("DUCAT_CALL_STATE") ?: error("CALL_FAIL set DUCAT_CALL_STATE"))
        .apply { mkdirs() }

    when (role) {
        // The answerphone: for a HUMAN (or emulator) caller — answers any
        // ring, streams the tone one way, counts what arrives without
        // judging it (a real microphone is not a deterministic sine).
        "answerphone" -> {
            val context = up(dir)
            NameStore(context).get() ?: NameStore(context).put("Answerphone")
            val card = Mailbox.issueCard(context, "Answerphone", 60uL * 60uL)
            println("CALL_CARD ${card.uri}")
            System.out.flush()
            val deadline = System.currentTimeMillis() + 1_800_000
            while (System.currentTimeMillis() < deadline) {
                runCatching { Mailbox.collectClaims(context) }
                runCatching { Mailbox.poll(context) }
                val store = ContactStore(context)
                val caller = store.all().firstOrNull()
                val offer = caller?.let { c ->
                    store.thread(c.personaHex).lastOrNull {
                        !it.outgoing && it.kind == 14 &&
                            // Fresh enough to answer: wider than the engine's
                            // ring window, because a cold mailbox can spend
                            // most of a minute delivering the offer itself.
                            System.currentTimeMillis() / 1000 - it.timestamp < 120
                    }
                }
                if (caller != null && offer?.callRoute != null && offer.callId != null) {
                    println("CALL_RINGING")
                    System.out.flush()
                    // One bad answer must not kill the answerphone — the
                    // next poll sees the same offer and tries again.
                    val mine = runCatching { nodeCallRoute() }
                        .onFailure { System.err.println("answer failed: ${it.message}") }
                        .getOrNull()
                    if (mine == null) {
                        Thread.sleep(2_000)
                        continue
                    }
                    val fresh = store.all().first { it.personaHex == caller.personaHex }
                    Mailbox.send(
                        context, fresh, "answer",
                        kind = 15, callRoute = mine,
                        callId = hexToBytes(offer.callId),
                    )
                    println("CALL_ANSWERED")
                    System.out.flush()
                    val route = hexToBytes(offer.callRoute)
                    var rx = 0
                    var decoded = 0
                    val rxT = Thread {
                        val end = System.currentTimeMillis() + 90_000
                        while (System.currentTimeMillis() < end) {
                            val f = nodeCallRecv(50u) ?: continue
                            rx++
                            if (f.size > 8 &&
                                runCatching { callDecode(f.copyOfRange(8, f.size)) }.isSuccess
                            ) {
                                decoded++
                            }
                        }
                    }.apply { start() }
                    // Answering side: wait to hear the caller before the tone
                    // starts — but a phone with a dead microphone still
                    // deserves to hear something, so give up after 30 s.
                    val hold = System.currentTimeMillis() + 30_000
                    while (rx == 0 && System.currentTimeMillis() < hold) Thread.sleep(20)
                    val t0 = System.currentTimeMillis()
                    for (seq in 0 until 3000) {
                        val due = t0 + seq * FRAME_MS
                        val wait = due - System.currentTimeMillis()
                        if (wait > 0) Thread.sleep(wait)
                        val pkt = runCatching { callEncode(tonePcm(seq)) }.getOrNull() ?: continue
                        runCatching { nodeCallSend(route, framed(seq, pkt)) }
                    }
                    rxT.join()
                    nodeCallClose()
                    println(
                        "ANSWERPHONE_STATS sent=3000 recv=$rx decoded=$decoded " +
                            "sendReport=${uniffi.ducat_mobile.nodeCallSendReport()}",
                    )
                    return
                }
                Thread.sleep(2_000)
            }
            error("CALL_FAIL nobody rang the answerphone")
        }
        // The callback: rings the contact it already has (an earlier
        // answerphone run's caller), so a human's phone can be on the
        // RECEIVING end — the bell, the Answer button, the Decline button.
        "callback" -> {
            val context = up(dir)
            val store = ContactStore(context)
            val callee = store.all().firstOrNull() ?: error("CALL_FAIL no contact to ring")
            runCatching { Mailbox.poll(context) }
            val mine = nodeCallRoute()
            val id = ByteArray(8).also { java.security.SecureRandom().nextBytes(it) }
            Mailbox.send(
                context, callee, "ring",
                kind = 14, callRoute = mine, callId = id,
            )
            val offerSeq = ContactStore(context).thread(callee.personaHex)
                .last { it.outgoing && it.kind == 14 && it.callId == id.toHexString() }
                .seq
            println("CALL_RINGING")
            System.out.flush()
            val waitSecs = (System.getenv("DUCAT_CB_WAIT") ?: "90").toLong()
            val deadline = System.currentTimeMillis() + waitSecs * 1000
            while (System.currentTimeMillis() < deadline) {
                runCatching { Mailbox.poll(context) }
                val thread = ContactStore(context).thread(callee.personaHex)
                val answer = thread.lastOrNull {
                    !it.outgoing && it.kind == 15 && it.callId == id.toHexString()
                }
                // Their retract with reOwn=false points at MY message — the offer.
                val declined = thread.any {
                    !it.outgoing && it.kind == 5 && it.reSeq == offerSeq && !it.reOwn
                }
                if (declined) {
                    println("CALLBACK_DECLINED")
                    return
                }
                if (answer?.callRoute != null) {
                    println("CALL_ANSWERED")
                    System.out.flush()
                    val route = hexToBytes(answer.callRoute)
                    var rx = 0
                    val rxT = Thread {
                        val end = System.currentTimeMillis() + 60_000
                        while (System.currentTimeMillis() < end) {
                            if (nodeCallRecv(50u) != null) rx++
                        }
                    }.apply { start() }
                    val t0 = System.currentTimeMillis()
                    for (seq in 0 until 2500) {
                        val due = t0 + seq * FRAME_MS
                        val wait = due - System.currentTimeMillis()
                        if (wait > 0) Thread.sleep(wait)
                        val pkt = runCatching { callEncode(tonePcm(seq)) }.getOrNull() ?: continue
                        runCatching { nodeCallSend(route, framed(seq, pkt)) }
                    }
                    rxT.join()
                    nodeCallClose()
                    println("CALLBACK_STATS sent=2500 recv=$rx")
                    return
                }
                Thread.sleep(2_000)
            }
            nodeCallClose()
            println("CALLBACK_UNANSWERED")
        }
        "callee" -> {
            val context = up(dir)
            NameStore(context).get() ?: NameStore(context).put("Callee")
            val card = Mailbox.issueCard(context, "Callee", 60uL * 60uL)
            println("CALL_CARD ${card.uri}")
            System.out.flush()

            val deadline = System.currentTimeMillis() + 600_000
            while (System.currentTimeMillis() < deadline) {
                runCatching { Mailbox.collectClaims(context) }
                runCatching { Mailbox.poll(context) }
                val store = ContactStore(context)
                val caller = store.all().firstOrNull()
                val offer = caller?.let { c ->
                    store.thread(c.personaHex).lastOrNull { !it.outgoing && it.kind == 14 }
                }
                if (caller != null && offer?.callRoute != null && offer.callId != null) {
                    println("CALL_RINGING id=${offer.callId.take(8)}…")
                    System.out.flush()
                    val mine = nodeCallRoute()
                    val fresh = store.all().first { it.personaHex == caller.personaHex }
                    Mailbox.send(
                        context, fresh, "answer",
                        kind = 15,
                        callRoute = mine,
                        callId = hexToBytes(offer.callId),
                    )
                    println("CALL_ANSWERED")
                    System.out.flush()
                    val ok = media(hexToBytes(offer.callRoute), initiator = false)
                    nodeCallClose()
                    if (ok) println("CALL_OK") else error("CALL_FAIL media did not verify")
                    return
                }
                Thread.sleep(2_000)
            }
            error("CALL_FAIL nobody rang")
        }
        "caller" -> {
            val cardUri = System.getenv("DUCAT_CALL_CARD") ?: error("CALL_FAIL set DUCAT_CALL_CARD")
            val context = up(dir)
            NameStore(context).get() ?: NameStore(context).put("Caller")
            val callee = ContactStore(context).all().firstOrNull()
                ?: Mailbox.claimCard(
                    context, uniffi.ducat_mobile.readContactCard(cardUri), "callee",
                )
            val mine = nodeCallRoute()
            val id = ByteArray(8).also { java.security.SecureRandom().nextBytes(it) }
            Mailbox.send(
                context, callee, "ring",
                kind = 14, callRoute = mine, callId = id,
            )
            println("CALL_RINGING")
            System.out.flush()

            val deadline = System.currentTimeMillis() + 600_000
            while (System.currentTimeMillis() < deadline) {
                runCatching { Mailbox.poll(context) }
                val answer = ContactStore(context).thread(callee.personaHex)
                    .lastOrNull { !it.outgoing && it.kind == 15 }
                if (answer?.callRoute != null &&
                    answer.callId == id.toHexString()
                ) {
                    println("CALL_ANSWERED")
                    System.out.flush()
                    val ok = media(hexToBytes(answer.callRoute), initiator = true)
                    nodeCallClose()
                    if (ok) println("CALL_OK") else error("CALL_FAIL media did not verify")
                    return
                }
                Thread.sleep(2_000)
            }
            error("CALL_FAIL no answer")
        }
        else -> error("CALL_FAIL unknown role $role")
    }
}

private fun hexToBytes(hex: String): ByteArray =
    ByteArray(hex.length / 2) {
        ((Character.digit(hex[it * 2], 16) shl 4) + Character.digit(hex[it * 2 + 1], 16)).toByte()
    }

private fun ByteArray.toHexString(): String = joinToString("") { "%02x".format(it) }
