package org.ducatproject.ducat

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.util.Calendar

/**
 * Recurring bills: a schedule, not a mandate.
 *
 * What repeats here is the *asking*. Each due date mints an ordinary
 * kind-1 PaymentRequest — the same message a hand-typed request sends —
 * and §16.13 already establishes that a request carries no authority: the
 * payer approves every one on the confirm screen, or ignores it, exactly
 * as if the landlord had typed it that morning. That is why auto-sending
 * is safe where auto-*paying* never would be, and why nothing about this
 * store touches the wire, the spec, or the other phone. Cancelling is
 * local for the same reason: stopping the asking needs nobody's
 * agreement.
 */
object Recurring {
    private const val TAG = "Recurring"

    data class Bill(
        val id: String,
        val personaHex: String,
        val amountPxmr: Long,
        val note: String,
        /** Calendar-monthly when true, else every seven days. */
        val monthly: Boolean,
        /** When the next request is owed, epoch millis. */
        val nextAt: Long,
    )

    // The schedule names who pays you what, on what cadence — the shape of
    // a business relationship. Same at-rest treatment as the contacts it
    // points into.
    private fun prefs(context: Context) = securePrefs(context, "ducat_recurring")

    /**
     * Guards read-modify-write of the schedule list, like every other store
     * that rewrites its array whole ([TabStore]'s is the one this copies).
     *
     * Without it the poller and a screen wrote from lists read before each
     * other's change, and the one that matters is the *stop*: cancelling a
     * subscription while [runDue] was mid-send meant the poller's write put
     * it back, with its next date advanced, and it billed somebody again a
     * month later. The user did the one thing they could do and the app
     * undid it silently.
     */
    private val lock = Any()

    private fun <T> guarded(f: () -> T): T = synchronized(lock) { f() }

    fun all(context: Context): List<Bill> {
        val raw = prefs(context).getString("bills", null) ?: return emptyList()
        return runCatching {
            val arr = JSONArray(raw)
            (0 until arr.length()).map { i ->
                val o = arr.getJSONObject(i)
                Bill(
                    id = o.getString("id"),
                    personaHex = o.getString("who"),
                    amountPxmr = o.getLong("amt"),
                    note = o.optString("note", ""),
                    monthly = o.getBoolean("m"),
                    nextAt = o.getLong("next"),
                )
            }
        }.getOrDefault(emptyList())
    }

    private fun save(context: Context, bills: List<Bill>) {
        val arr = JSONArray()
        bills.forEach { b ->
            arr.put(
                JSONObject()
                    .put("id", b.id)
                    .put("who", b.personaHex)
                    .put("amt", b.amountPxmr)
                    .put("note", b.note)
                    .put("m", b.monthly)
                    .put("next", b.nextAt),
            )
        }
        prefs(context).edit().putString("bills", arr.toString()).apply()
        ContactStore.bump()
    }

    /** Register a schedule whose first request was just sent by hand:
     *  the next one is owed a full period from now. */
    fun add(context: Context, personaHex: String, amountPxmr: Long, note: String, monthly: Boolean) {
        val now = System.currentTimeMillis()
        val b = Bill(
            id = java.util.UUID.randomUUID().toString(),
            personaHex = personaHex,
            amountPxmr = amountPxmr,
            note = note,
            monthly = monthly,
            nextAt = advance(now, monthly),
        )
        guarded { save(context, all(context) + b) }
        DucatLog.i(TAG, "scheduled ${if (monthly) "monthly" else "weekly"} bill for ${personaHex.take(8)}")
    }

    fun stop(context: Context, id: String) =
        guarded { save(context, all(context).filterNot { it.id == id }) }

    private fun advance(from: Long, monthly: Boolean): Long =
        if (monthly) {
            // Calendar arithmetic, not day-counting: rent asked on the 31st
            // lands on the 30th where a month is short, not on the 3rd.
            Calendar.getInstance().apply {
                timeInMillis = from
                add(Calendar.MONTH, 1)
            }.timeInMillis
        } else {
            from + 7L * 24 * 60 * 60 * 1000
        }

    /**
     * The poller's hook: send what has come due.
     *
     * One request per bill per pass, advancing one period on success — so
     * a phone dark for three months surfaces three bills over three
     * passes, minutes apart, rather than skipping months that are still
     * owed or dumping them in one burst. On failure nothing advances and
     * nothing is lost: the same request is owed on the next pass.
     */
    fun runDue(context: Context) {
        val now = System.currentTimeMillis()
        val bills = all(context)
        if (bills.none { it.nextAt <= now }) return
        // Read once, not once per bill: `all()` decrypts the whole book.
        val book = ContactStore(context).all()
        val advanced = HashMap<String, Long>()
        val forgotten = HashSet<String>()
        for (b in bills) {
            if (b.nextAt > now) continue
            val c = book.firstOrNull { it.personaHex == b.personaHex }
            if (c == null) {
                // The contact was forgotten; a bill to nobody stops itself.
                DucatLog.w(TAG, "recurring bill points at a forgotten contact; dropping")
                forgotten += b.id
                continue
            }
            runCatching {
                Mailbox.send(
                    context, c,
                    b.note.ifBlank { context.getString(R.string.pay_payment_request) },
                    kind = 1, amountPxmr = b.amountPxmr,
                    payto = WalletStore(context).addressFor(b.personaHex),
                )
            }.fold(
                onSuccess = {
                    Notify.post(
                        context, c.displayName(),
                        context.getString(
                            R.string.pay_recur_notify,
                            Amounts.show(context, b.amountPxmr).primary,
                        ),
                        openChat = b.personaHex,
                    )
                    advanced[b.id] = advance(b.nextAt, b.monthly)
                },
                onFailure = { DucatLog.w(TAG, "recurring bill not sent: ${it.message}") },
            )
        }
        if (advanced.isEmpty() && forgotten.isEmpty()) return
        // **Written against the list as it stands now, not the snapshot the
        // sending started from.** A request is network-seconds, and in that
        // time somebody may have stopped a schedule or added one; writing
        // the whole snapshot back put the stopped one on the books again —
        // with its date advanced, so it billed again next period — and
        // dropped the new one. Only the schedules this pass actually acted
        // on are touched, by id, and a schedule that is no longer there is
        // simply not put back.
        guarded {
            val cur = all(context)
            val next = applyRun(cur, advanced, forgotten)
            if (next != cur) save(context, next)
        }
    }

    /**
     * The list a finished pass leaves behind, given the list as it stands
     * *now* and what the pass actually did.
     *
     * Pulled out because it is the whole of the fix and none of it needs a
     * network: a schedule stopped while its request was in flight must not
     * come back, one added meanwhile must survive, and only the ones this
     * pass really sent may move their date. The old code wrote its own
     * snapshot back over all three.
     */
    internal fun applyRun(
        current: List<Bill>,
        advanced: Map<String, Long>,
        forgotten: Set<String>,
    ): List<Bill> = current.mapNotNull { b ->
        when {
            b.id in forgotten -> null
            advanced.containsKey(b.id) -> b.copy(nextAt = advanced.getValue(b.id))
            else -> b
        }
    }
}
