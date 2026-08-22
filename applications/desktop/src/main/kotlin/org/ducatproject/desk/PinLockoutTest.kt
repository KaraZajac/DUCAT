package org.ducatproject.desk

import org.ducatproject.ducat.Pin

/**
 * A cooldown the date picker cannot skip. `./gradlew :desktop:pinlockout`.
 *
 * Everything else about the PIN is sound — PBKDF2 at 200k, a constant-time
 * compare, four free tries then doubling to fifteen minutes. All of which is
 * worth nothing if the deadline is written against a clock that the person
 * holding the phone can set. Push the date a year forward and every cooldown
 * ever written is in the past, and a four-digit PIN is then only as expensive
 * as PBKDF2 will run: ten thousand guesses in well under an hour.
 *
 * So there are two clocks and the longer answer wins. What follows is that
 * arithmetic, which has to get three things right at once — the attack, the
 * reboot, and the honest wait.
 */
fun main() {
    val minute = 60_000L
    val cap = 15 * minute      // Pin.MAX_COOLDOWN_MS

    // Nothing owed on either clock.
    check(Pin.remaining(0, 0) == 0L) { "PINLOCK_FAIL zero is not zero" }
    check(Pin.remaining(-minute, -minute) == 0L) { "PINLOCK_FAIL past deadlines" }

    // The attack: wall clock shoved a year into the future, so the wall
    // deadline is long past. The monotonic one has not moved and still owes
    // ten minutes, and that is the answer.
    check(Pin.remaining(-365 * 24 * 60 * minute, 10 * minute) == 10 * minute) {
        "PINLOCK_FAIL moving the date forward skipped the cooldown"
    }

    // The same trick backwards, in case the two are ever swapped: whichever
    // clock still owes time is the one that counts.
    check(Pin.remaining(10 * minute, -minute) == 10 * minute) {
        "PINLOCK_FAIL a stale monotonic deadline cancelled a live wall one"
    }

    // The reboot. elapsedRealtime restarts near zero, so a deadline written
    // after three days of uptime reads as three days still to wait. Capped at
    // one full cooldown — a reboot is not an escape and not a brick either.
    check(Pin.remaining(-minute, 3 * 24 * 60 * minute) == cap) {
        "PINLOCK_FAIL a reboot locked the phone for longer than any real cooldown"
    }
    // Same cap the other way, so nobody bricks themselves with the date.
    check(Pin.remaining(3 * 24 * 60 * minute, 0) == cap) {
        "PINLOCK_FAIL a wall deadline in the far future was believed"
    }

    // And the ordinary case, which all of the above must not disturb: both
    // clocks agree there are thirty seconds left.
    check(Pin.remaining(30_000, 30_000) == 30_000L) {
        "PINLOCK_FAIL an honest wait was not reported"
    }
    // Slight skew between them is normal and takes the longer.
    check(Pin.remaining(29_000, 30_000) == 30_000L) { "PINLOCK_FAIL skew" }

    println("PINLOCK_OK attack=held reboot=capped honest=kept cap=${cap / minute}min")
}
