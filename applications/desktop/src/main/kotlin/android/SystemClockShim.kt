package android.os

/**
 * The clock nobody can set.
 *
 * On the phone this counts milliseconds since boot and no setting reaches it,
 * which is why Pin measures its lockout against it as well as against the wall
 * clock — a cooldown that the date picker can skip is not a cooldown.
 *
 * The JVM's nearest equivalent is nanoTime, whose origin is arbitrary but
 * whose direction is guaranteed. That is the whole of what is wanted here:
 * an interval that cannot be argued with.
 */
object SystemClock {
    @JvmStatic
    fun elapsedRealtime(): Long = System.nanoTime() / 1_000_000

    @JvmStatic
    fun uptimeMillis(): Long = elapsedRealtime()
}
