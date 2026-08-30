package org.ducatproject.ducat

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat

/**
 * Telling the user something happened while they were not looking.
 *
 * A payments app that only speaks when open is a payments app checked like a
 * mailbox; every peer here notifies on arrival, and the absence reads as
 * broken. The events are exactly the ones a person would interrupt a
 * conversation for: a message, a bill, money in, a receipt.
 *
 * **What reaches the lock screen is the fact, never the figure.** Every
 * notification is `VISIBILITY_PRIVATE` with a public fallback of just the app
 * name and "activity" — an amount on a lock screen is a balance hint to anyone
 * who glances at the table, and this is a privacy app before it is a
 * convenient one. The full text waits behind the unlock.
 */
object Notify {
    private const val CHANNEL = "ducat_activity"
    private var nextId = 1000

    private fun manager(context: Context): NotificationManager? {
        val mgr = context.getSystemService(NotificationManager::class.java) ?: return null
        mgr.createNotificationChannel(
            NotificationChannel(
                CHANNEL, context.getString(R.string.notify_channel_name),
                NotificationManager.IMPORTANCE_DEFAULT,
            ).apply {
                description = context.getString(R.string.notify_channel_desc)
            }
        )
        return mgr
    }

    fun post(context: Context, title: String, body: String, openChat: String? = null) {
        // Android 13+ gates posting behind a runtime permission; posting
        // without it throws on some builds and is silently dropped on others.
        // Either way the caller cannot fix it here, so check and skip.
        if (android.os.Build.VERSION.SDK_INT >= 33 &&
            context.checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) !=
            android.content.pm.PackageManager.PERMISSION_GRANTED
        ) return

        val mgr = manager(context) ?: return
        val open = PendingIntent.getActivity(
            context, ++reqCode,
            Intent(context, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_SINGLE_TOP
                openChat?.let { putExtra("open_chat", it) }
            },
            // The extra varies per notification; without UPDATE_CURRENT every
            // notification reuses the first one's intent and every tap lands
            // in the same thread.
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val fact = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_cat_notify)
            .setContentTitle(context.getString(R.string.app_name))
            .setContentText(context.getString(R.string.notify_public_activity))
            .build()
        val full = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_cat_notify)
            .setContentTitle(title)
            .setContentText(body)
            .setStyle(NotificationCompat.BigTextStyle().bigText(body))
            .setContentIntent(open)
            .setAutoCancel(true)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setPublicVersion(fact)
            .build()
        mgr.notify(nextId++, full)
    }

    /** Distinct request codes so two threads' taps do not share one intent. */
    private var reqCode = 100

    /** An inbound thread message, worded by what it is (§16.13's kinds). */
    fun message(context: Context, from: String, personaHex: String, m: StoredMessage) {
        val what = when (m.kind) {
            1 -> {
                // Their language, not ours: the placeholder body a bill
                // carries when nobody wrote a note is in the sender's.
                val filler = Languages.everyTranslationOf(
                    context, R.string.pay_payment_request,
                )
                val note = m.body.takeIf { it.isNotBlank() && it !in filler }
                if (note != null) context.getString(
                    R.string.notify_asks_for_note, from, xmr(context, m.amountPxmr), note)
                else context.getString(
                    R.string.notify_asks_for, from, xmr(context, m.amountPxmr))
            }
            2 -> context.getString(R.string.notify_sent, from, xmr(context, m.amountPxmr))
            3 -> context.getString(R.string.notify_receipt, from, xmr(context, m.amountPxmr))
            // §17.9's ceremony traffic. The body of one of these is a note the
            // *sender's* phone wrote for the protocol — "ride: proposed a
            // split" — and falling through to it put that on a lock screen, in
            // English, above a bike repair, at the exact moment the reader was
            // being asked to release their money. Every other kind here has
            // been rendered by the reader for a while; these two were the ones
            // nobody had met yet.
            8 -> context.getString(R.string.notify_escrow_setup, from)
            9 -> context.getString(R.string.notify_escrow_settle, from)
            10 -> context.getString(R.string.notify_escrow_called_off, from)
            else -> m.body
        }
        // Which compartment this reached, said in the title once a second
        // persona exists: three shops on one phone cannot share an
        // undifferentiated "Sam paid you".
        val personas = PersonaStore(context)
        val title = if (personas.all().size > 1) {
            val owner = ContactStore(context).all()
                .firstOrNull { it.personaHex == personaHex }
                ?.let { personas.ownerHexOf(it) }
            val label = personas.all().firstOrNull { it.hex == owner }
                ?.name?.ifBlank { null }
                ?: context.getString(R.string.personas_primary)
            "$label · $from"
        } else from
        post(context, title, what, openChat = personaHex)
    }

    /**
     * An amount as this phone reads money.
     *
     * A notification is the one place a payment is met without a screen around
     * it — no balance above, nothing to compare against — so "0.028571 XMR"
     * arriving on the lock screen tells the person almost nothing. It was the
     * last thing still quoting piconero at somebody who had not asked for it.
     */
    private fun xmr(context: Context, pxmr: Long) = Amounts.show(context, pxmr).primary
}
