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
        save(context, all(context) + b)
        DucatLog.i(TAG, "scheduled ${if (monthly) "monthly" else "weekly"} bill for ${personaHex.take(8)}")
    }

    fun stop(context: Context, id: String) {
        save(context, all(context).filterNot { it.id == id })
    }

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
        val store = ContactStore(context)
        val mine = PersonaStore(context).personaHex()
        val out = bills.map { b ->
            if (b.nextAt > now) return@map b
            val c = store.all().firstOrNull { it.personaHex == b.personaHex }
            if (c == null) {
                // The contact was forgotten; a bill to nobody stops itself.
                DucatLog.w(TAG, "recurring bill points at a forgotten contact; dropping")
                return@map null
            }
            val sent = runCatching {
                Mailbox.send(
                    context, c,
                    b.note.ifBlank { context.getString(R.string.pay_payment_request) },
                    mine,
                    kind = 1, amountPxmr = b.amountPxmr,
                    payto = WalletStore(context).addressFor(b.personaHex),
                )
            }
            sent.fold(
                onSuccess = {
                    Notify.post(
                        context, c.displayName(),
                        context.getString(
                            R.string.pay_recur_notify,
                            Amounts.show(context, b.amountPxmr).primary,
                        ),
                        openChat = b.personaHex,
                    )
                    b.copy(nextAt = advance(b.nextAt, b.monthly))
                },
                onFailure = {
                    DucatLog.w(TAG, "recurring bill not sent: ${it.message}")
                    b
                },
            )
        }.filterNotNull()
        if (out != bills) save(context, out)
    }
}
