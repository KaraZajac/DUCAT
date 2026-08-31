package org.ducatproject.desk

import java.io.File
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.sin
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.NameStore
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
 * v0 media: 8-byte header (seq u32be ‖ ms u32be) + 20 ms of PCM16 mono
 * 16 kHz — 640 payload bytes at 50 Hz, each frame a deterministic 440 Hz
 * sine slice so the receiver can verify every byte it hears.
 *
 *   DUCAT_CALL_ROLE=callee DUCAT_CALL_STATE=<dir>
 *   DUCAT_CALL_ROLE=caller DUCAT_CALL_STATE=<dir> DUCAT_CALL_CARD=<uri>
 *
 * Markers: CALL_CARD (callee's), CALL_RINGING, CALL_ANSWERED, then each
 * side prints CALL_STATS sent=N recv=N loss=N badbytes=N jitter=Nms and
 * CALL_OK when the far side's audio verified.
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

/** One deterministic frame of the tone: this seq's 20 ms of 440 Hz. */
private fun frameBytes(seq: Int): ByteArray {
    val out = ByteArray(8 + SAMPLES * 2)
    out[0] = (seq ushr 24).toByte(); out[1] = (seq ushr 16).toByte()
    out[2] = (seq ushr 8).toByte(); out[3] = seq.toByte()
    val ms = seq * FRAME_MS
    out[4] = (ms ushr 24).toByte(); out[5] = (ms ushr 16).toByte()
    out[6] = (ms ushr 8).toByte(); out[7] = ms.toByte()
    for (i in 0 until SAMPLES) {
        val t = (seq * SAMPLES + i).toDouble() / 16_000.0
        val v = (sin(2 * PI * 440.0 * t) * 12_000).toInt()
        out[8 + i * 2] = (v and 0xFF).toByte()
        out[9 + i * 2] = ((v shr 8) and 0xFF).toByte()
    }
    return out
}

private fun media(theirRoute: ByteArray): Boolean {
    var sent = 0
    val recvSeqs = HashSet<Int>()
    var badBytes = 0
    val arrivals = ArrayList<Long>(FRAMES)

    // The receiver drains on its own thread — full duplex, like a call.
    val rx = Thread {
        val end = System.currentTimeMillis() + FRAMES * FRAME_MS + 5_000
        while (System.currentTimeMillis() < end) {
            val f = nodeCallRecv(50u) ?: continue
            arrivals.add(System.currentTimeMillis())
            if (f.size != 8 + SAMPLES * 2) { badBytes++; continue }
            val seq = ((f[0].toInt() and 0xFF) shl 24) or ((f[1].toInt() and 0xFF) shl 16) or
                ((f[2].toInt() and 0xFF) shl 8) or (f[3].toInt() and 0xFF)
            if (!frameBytes(seq).contentEquals(f)) badBytes++
            recvSeqs.add(seq)
        }
    }.apply { start() }

    val t0 = System.currentTimeMillis()
    for (seq in 0 until FRAMES) {
        val due = t0 + seq * FRAME_MS
        val wait = due - System.currentTimeMillis()
        if (wait > 0) Thread.sleep(wait)
        runCatching { nodeCallSend(theirRoute, frameBytes(seq)) }
            .onSuccess { sent++ }
            .onFailure { if (seq % 100 == 0) System.err.println("send $seq: ${it.message}") }
    }
    rx.join()

    val loss = FRAMES - recvSeqs.size
    // RFC 3550-flavoured jitter on the arrival cadence.
    var jitter = 0.0
    for (i in 1 until arrivals.size) {
        jitter += abs((arrivals[i] - arrivals[i - 1]) - FRAME_MS).toDouble()
    }
    if (arrivals.size > 1) jitter /= (arrivals.size - 1)
    println(
        "CALL_STATS sent=$sent recv=${recvSeqs.size} loss=$loss " +
            "badbytes=$badBytes jitter=${"%.1f".format(jitter)}ms",
    )
    return loss < FRAMES / 20 && badBytes == 0 // ≤5% loss, every byte true
}

fun main() {
    val role = System.getenv("DUCAT_CALL_ROLE") ?: error("CALL_FAIL set DUCAT_CALL_ROLE")
    val dir = File(System.getenv("DUCAT_CALL_STATE") ?: error("CALL_FAIL set DUCAT_CALL_STATE"))
        .apply { mkdirs() }

    when (role) {
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
                    val ok = media(hexToBytes(offer.callRoute))
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
                    val ok = media(hexToBytes(answer.callRoute))
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
