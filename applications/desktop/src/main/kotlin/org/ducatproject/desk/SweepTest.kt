package org.ducatproject.desk

import org.ducatproject.ducat.TabStore

/**
 * What the abandoned-tab sweep is allowed to take, pinned.
 *
 * A tab is committed before its bill goes out on purpose — Orders.bind
 * takes "a re-billable open tab" over an order with no tab at all — and
 * nothing ever collected the ones left behind by a send that failed. The
 * sweep collects them, which makes it the one piece of this store that
 * deletes something nobody asked it to delete.
 *
 * So the interesting cases are all the ones it must *not* take: the bar's
 * running account, which is meant to sit open all evening; a tab an order
 * or a subscription still reads through its id; anything billed, whatever
 * its origin; and a tab stamped in the future, where the house rule for
 * every other timer here ([org.ducatproject.ducat.Elapsed]) would say
 * "due" and cost a live tab.
 *
 * `DUCAT_DESK_STATE=<dir> ./gradlew :desktop:sweeptest`.
 */
fun main() {
    var failures = 0
    fun check(name: String, ok: Boolean, detail: String = "") {
        println("SWEEP ${if (ok) "ok  " else "FAIL"} $name${if (detail.isEmpty()) "" else " — $detail"}")
        if (!ok) failures++
    }

    val dir = kotlin.io.path.createTempDirectory("ducat-sweep").toFile()
    val store = TabStore(DeskContext(dir))
    val now = 1_700_000_000_000L
    val old = now - TabStore.ABANDONED_MS - 1
    val sam = "aa".repeat(32)

    // Ages are set by rewriting openedAt: open() stamps the wall clock, and
    // this test cannot wait a day.
    fun tab(origin: String, openedAt: Long, state: String = "open"): String {
        val t = store.open(sam, origin)
        store.mutate(t.id) { it.copy(openedAt = openedAt, state = state) }
        return t.id
    }

    val staleSale = tab("pos", old)
    val freshSale = tab("pos", now - 1000)
    val staleBar = tab("bar", old)
    val staleOrder = tab(org.ducatproject.ducat.Orders.ORIGIN, old)
    val staleBilled = tab("pos", old, state = "settled")
    // A clock wound forward and back: this stamp is a day and a half ahead
    // of now, which Elapsed.due would read as due.
    val future = tab("pos", now + TabStore.ABANDONED_MS + TabStore.ABANDONED_MS / 2)

    // A taxi whose bill failed to send: settle's catch puts the tab back to
    // "open" and leaves the ride running, so a driver who was offline at the
    // kerb has an open taxi tab with a live meter behind it. The sweep sees
    // "open", not "bar", older than a day — and deleting it takes the fare
    // while the ride screen still shows one. The poller passes it in the
    // keep-set; this is that contract.
    val liveRide = tab("taxi", old)

    val took = store.sweepAbandoned(now, keep = setOf(staleOrder, liveRide))
    val left = store.all().map { it.id }.toSet()

    check("the abandoned counter sale goes", staleSale !in left)
    check("one tab taken, not more", took == 1, "took $took")
    check("a sale from a minute ago stays", freshSale in left)
    check("the bar's running account stays", staleBar in left)
    check("a tab an order still reads stays", staleOrder in left)
    check("and the tab a running ride is billing through", liveRide in left)
    check("a billed tab stays, whatever its origin", staleBilled in left)
    check("a stamp from the future stays", future in left)

    // Idempotent: nothing left to take, and the second pass says so rather
    // than reporting work it did not do.
    check("a second pass takes nothing", store.sweepAbandoned(now, keep = setOf(staleOrder, liveRide)) == 0)

    // A day later the fresh one has aged into debris, and the guarded ones
    // have not moved.
    val later = now + TabStore.ABANDONED_MS + 1
    check(
        "the sale ages into the sweep",
        store.sweepAbandoned(later, keep = setOf(staleOrder, liveRide)) == 1,
    )
    val end = store.all().map { it.id }.toSet()
    check("and the guards still hold a day on", end == setOf(staleBar, staleOrder, staleBilled, future, liveRide))

    // Dropping the keep-set is what an order being finished looks like.
    check("an unreferenced order tab goes once nothing reads it",
        store.sweepAbandoned(later, keep = emptySet()) == 2)
    check("leaving the bar and the billed tab", store.all().map { it.id }.toSet() == setOf(staleBar, staleBilled, future))

    if (failures > 0) {
        println("SWEEPTEST FAILED — $failures")
        kotlin.system.exitProcess(1)
    }
    println("SWEEPTEST OK")
}
