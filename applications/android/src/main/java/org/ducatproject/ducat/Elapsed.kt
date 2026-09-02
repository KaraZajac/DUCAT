package org.ducatproject.ducat

/**
 * "Has this waited long enough?", asked so that a clock which has moved
 * cannot answer *no* for ever.
 *
 * Every timer in the app is the same subtraction — `now - stamp >= window`
 * — and every one of them has the same hole in it: a stamp *ahead* of now
 * makes the difference negative, so the window never passes and the thing
 * never happens again. Not a hypothetical. A phone's clock is wound
 * forward (the weekly-epoch tests do exactly that), something is stamped
 * under the wrong date, the clock comes back, and from then on the listing
 * is never re-posted, the intent is never given up on, the card is never
 * retired, the nudge is never sent. Nothing looks wrong: the record says
 * it was written a moment ago, in the future.
 *
 * The rate cache found this live on 2026-09-02 — one phone priced XMR at
 * 461 against a market at 520, confidently, for five days
 * ([RateStore.isStale], where the reasoning is written out) — and the same
 * shape was sitting in a dozen other places. This is that rule, once, for
 * all of them: a stamp this phone cannot vouch for reads as *due*, because
 * doing the thing once too often costs a round trip and never doing it
 * again costs the feature.
 *
 * The slack is the clock nudged back by a couple of seconds between the
 * write and the read, which is ordinary and must not trip this.
 *
 * Two places worked this out for themselves before it was a rule and keep
 * their own wording, because each explains what it costs where it is:
 * [RateStore.isStale] and [Beacons.usableTip]. Anything new belongs here.
 */
object Elapsed {
    /** How far ahead of now a stamp may sit and still be believed. */
    const val FUTURE_SLACK_MS = 60_000L
    const val FUTURE_SLACK_SECS = 60L

    /** Milliseconds. True when [stamp] is older than [window] — or ahead of
     *  [now], which no honest stamp from this device can be. */
    fun due(now: Long, stamp: Long, window: Long): Boolean {
        val age = now - stamp
        return age >= window || age < -FUTURE_SLACK_MS
    }

    /** The same, for the second-resolution stamps the protocol uses. */
    fun dueSecs(nowSecs: Long, stampSecs: Long, windowSecs: Long): Boolean {
        val age = nowSecs - stampSecs
        return age >= windowSecs || age < -FUTURE_SLACK_SECS
    }
}
