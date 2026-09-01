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
    // Posted from several poller lanes at once; a shared id would make one
    // notification silently replace another.
    private val nextId = java.util.concurrent.atomic.AtomicInteger(1000)

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
            context, reqCode.incrementAndGet(),
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
        mgr.notify(nextId.getAndIncrement(), full)
    }

    /** Distinct request codes so two threads' taps do not share one intent. */
    private val reqCode = java.util.concurrent.atomic.AtomicInteger(100)

    /** An inbound thread message, worded by what it is (§16.13's kinds). */
    fun message(context: Context, from: String, personaHex: String, m: StoredMessage) {
        // The answer to a ring this phone is on: the call screen is showing
        // exactly that, and "Jordan answered your call" beside "In a call
        // with Jordan" in the shade was the record announcing itself. An
        // answer that lands after the ring was given up on is news — they
        // picked up, you were gone — and still posts.
        if (m.kind == 15) {
            val onIt = when (val s = Calls.state) {
                is Calls.State.Outgoing -> s.contactHex == personaHex
                is Calls.State.Active -> s.contactHex == personaHex
                else -> false
            }
            if (onIt) return
        }
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
            // A publication key (§16.20): the body is the publisher's note or
            // nothing at all, and an empty notification reads as broken.
            13 -> context.getString(R.string.notify_new_issue, from)
            // §16.21: the ring that arrives while you were away IS the
            // missed-call notification — no second channel to build.
            14 -> context.getString(R.string.notify_call, from)
            15 -> context.getString(R.string.notify_call_answered, from)
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

    // ----- §16.21: the takeover an incoming call is owed -----

    private const val CALL_CHANNEL = "ducat_call"

    /** One ring at a time, so one fixed id — cancel is exact. */
    private const val CALL_NOTIFICATION_ID = 77

    /**
     * The full-screen ask: on a lit, unlocked phone a heads-up banner; on a
     * dark or locked one, Android launches the activity itself — which shows
     * [org.ducatproject.ducat.ui.CallScreen], because a live call outranks
     * every screen. The channel is deliberately silent and still: the bell
     * and the buzz are the engine's own (the British ring), and a channel
     * sound would ring twice. The lock screen learns there is a call, not
     * from whom — same rule as money.
     */
    fun ringIncoming(context: Context, from: String) {
        if (android.os.Build.VERSION.SDK_INT >= 33 &&
            context.checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) !=
            android.content.pm.PackageManager.PERMISSION_GRANTED
        ) return
        val mgr = context.getSystemService(NotificationManager::class.java) ?: return
        mgr.createNotificationChannel(
            NotificationChannel(
                CALL_CHANNEL, context.getString(R.string.notify_call_channel),
                NotificationManager.IMPORTANCE_HIGH,
            ).apply {
                setSound(null, null)
                enableVibration(false)
            }
        )
        val open = PendingIntent.getActivity(
            context, reqCode.incrementAndGet(),
            Intent(context, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP
            },
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val fact = NotificationCompat.Builder(context, CALL_CHANNEL)
            .setSmallIcon(R.drawable.ic_cat_notify)
            .setContentTitle(context.getString(R.string.app_name))
            .setContentText(context.getString(R.string.notify_call_incoming))
            .build()
        val full = NotificationCompat.Builder(context, CALL_CHANNEL)
            .setSmallIcon(R.drawable.ic_cat_notify)
            .setContentTitle(from)
            .setContentText(context.getString(R.string.notify_call_incoming))
            .setCategory(NotificationCompat.CATEGORY_CALL)
            .setPriority(NotificationCompat.PRIORITY_MAX)
            .setOngoing(true)
            .setContentIntent(open)
            .setFullScreenIntent(open, true)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setPublicVersion(fact)
            .build()
        mgr.notify(CALL_NOTIFICATION_ID, full)
    }

    /** The ring ended — answered, declined, expired, or withdrawn. */
    fun quietIncoming(context: Context) {
        context.getSystemService(NotificationManager::class.java)
            ?.cancel(CALL_NOTIFICATION_ID)
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
