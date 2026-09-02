package org.ducatproject.desk

import org.ducatproject.ducat.Recurring

/**
 * What a finished pass of recurring bills leaves behind, pinned.
 *
 * `runDue` reads the schedules, spends network-seconds sending a request
 * for each one that has come due, and then writes the list back. It used to
 * write back the list it *started* from, which is a lost write with a
 * month's consequence: somebody cancels a subscription while the poller is
 * mid-send, the poller's write puts it back with its date advanced, and it
 * bills them again next period. The user did the one thing they could do
 * and the app undid it, silently.
 *
 * The sending needs a network; deciding what to keep does not, which is why
 * that decision is its own function and this drives it directly.
 * `./gradlew :desktop:recurtest`.
 */
fun main() {
    var failures = 0
    fun check(name: String, ok: Boolean, detail: String = "") {
        println("RECUR ${if (ok) "ok  " else "FAIL"} $name${if (detail.isEmpty()) "" else " — $detail"}")
        if (!ok) failures++
    }

    fun bill(id: String, at: Long) = Recurring.Bill(
        id = id, personaHex = "aa".repeat(32), amountPxmr = 1_000L,
        note = "rent", monthly = true, nextAt = at,
    )

    val week = 7L * 24 * 60 * 60 * 1000
    val a = bill("a", 100L)
    val b = bill("b", 200L)

    // The ordinary pass: both were sent, both move on.
    run {
        val out = Recurring.applyRun(listOf(a, b), mapOf("a" to 999L, "b" to 888L), emptySet())
        check("a sent schedule moves to its next date",
            out.first { it.id == "a" }.nextAt == 999L)
        check("and so does the other", out.first { it.id == "b" }.nextAt == 888L)
        check("nothing is dropped", out.size == 2)
    }

    // **The race this exists for.** "a" was sent; while it was sending,
    // somebody stopped it. The list as it stands now no longer has it.
    run {
        val out = Recurring.applyRun(listOf(b), mapOf("a" to 999L), emptySet())
        check("a schedule stopped mid-send stays stopped",
            out.none { it.id == "a" }, "it used to come back with its date advanced")
        check("and the one left alone is untouched",
            out.singleOrNull()?.nextAt == 200L)
    }

    // The other half: one added while the pass was running.
    run {
        val fresh = bill("c", 300L)
        val out = Recurring.applyRun(listOf(a, b, fresh), mapOf("a" to 999L), emptySet())
        check("a schedule added mid-send survives the write-back",
            out.any { it.id == "c" && it.nextAt == 300L })
        check("one that was not sent keeps its date",
            out.first { it.id == "b" }.nextAt == 200L)
    }

    // A bill whose contact is gone stops itself — and only that one.
    run {
        val out = Recurring.applyRun(listOf(a, b), emptyMap(), setOf("a"))
        check("a bill to a forgotten contact is dropped", out.none { it.id == "a" })
        check("its neighbour is not", out.singleOrNull()?.id == "b")
    }

    // A pass that sent nothing changes nothing.
    run {
        val cur = listOf(a, b)
        check("an empty pass is the identity",
            Recurring.applyRun(cur, emptyMap(), emptySet()) == cur)
    }

    check("a week is seven days", week == 604_800_000L)
    println(if (failures == 0) "RECURTEST OK" else "RECURTEST FAILED ($failures)")
    if (failures > 0) kotlin.system.exitProcess(1)
}
