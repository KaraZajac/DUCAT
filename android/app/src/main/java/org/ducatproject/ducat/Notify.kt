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
                CHANNEL, "Messages & payments",
                NotificationManager.IMPORTANCE_DEFAULT,
            ).apply {
                description = "A message, a bill, a receipt, or money arriving."
            }
        )
        return mgr
    }

    fun post(context: Context, title: String, body: String) {
        // Android 13+ gates posting behind a runtime permission; posting
        // without it throws on some builds and is silently dropped on others.
        // Either way the caller cannot fix it here, so check and skip.
        if (android.os.Build.VERSION.SDK_INT >= 33 &&
            context.checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) !=
            android.content.pm.PackageManager.PERMISSION_GRANTED
        ) return

        val mgr = manager(context) ?: return
        val open = PendingIntent.getActivity(
            context, 0,
            Intent(context, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_SINGLE_TOP
            },
            PendingIntent.FLAG_IMMUTABLE,
        )
        val fact = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_ducat_mono)
            .setContentTitle("DUCAT")
            .setContentText("Activity")
            .build()
        val full = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_ducat_mono)
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

    /** An inbound thread message, worded by what it is (§16.13's kinds). */
    fun message(context: Context, from: String, m: StoredMessage) {
        val what = when (m.kind) {
            1 -> "$from asks for ${xmr(m.amountPxmr)}" +
                (m.body.takeIf { it.isNotBlank() && it != "Payment request" }
                    ?.let { " — $it" } ?: "")
            2 -> "$from sent ${xmr(m.amountPxmr)}"
            3 -> "Receipt from $from — ${xmr(m.amountPxmr)}"
            else -> m.body
        }
        post(context, from, what)
    }

    private fun xmr(pxmr: Long) = "${formatXmr(pxmr)} XMR"
}
