package org.ducatproject.ducat

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat

/**
 * Keeps the Veilid node alive when DUCAT is not on screen.
 *
 * Without this, Android is free to stop the process the moment the app is
 * backgrounded — and stopping the process destroys the node, which destroys
 * every private route, which makes every card handed out unclaimable and every
 * contact unreachable. Being reachable is not a background nicety for a
 * peer-to-peer app; it is the product.
 *
 * A **foreground** service, which means a permanent notification. That is the
 * deal Android offers and it is the honest one: an app holding the network open
 * should say so rather than doing it quietly.
 *
 * This does not make DUCAT reachable while the phone is in deep doze, and
 * nothing short of a push service would. What it buys is reachability while the
 * screen is off and the app is not foreground, which is most of a day.
 */
class NodeService : Service() {

    companion object {
        private const val CHANNEL = "ducat_node"
        private const val ID = 1
        /** Who the phone is in a call with, or absent: the plain node. */
        private const val EXTRA_IN_CALL_WITH = "in_call_with"
        private const val EXTRA_RINGING = "ringing"
        /** The notification's Hang up — the one button the call has when
         *  the app is off the screen. */
        private const val ACTION_HANG_UP = "org.ducatproject.ducat.HANG_UP"

        fun start(context: Context) = start(context, Intent(context, NodeService::class.java))

        private fun start(context: Context, i: Intent) {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(i)
            } else {
                context.startService(i)
            }
        }

        /**
         * A call is up with [from]. The same service, re-declared with the
         * microphone type: Android 14 lets a background app keep recording
         * only under a foreground service that says so, and without it the
         * far side goes silent the moment the screen turns off. Must be
         * called while the app is visible — that is the type's rule — which
         * a call that just connected always is.
         */
        fun inCall(context: Context, from: String, ringing: Boolean = false) = start(
            context,
            Intent(context, NodeService::class.java)
                .putExtra(EXTRA_IN_CALL_WITH, from)
                .putExtra(EXTRA_RINGING, ringing),
        )

        /** The call ended: back to the plain node notification. */
        fun callEnded(context: Context) = start(context)

        fun stop(context: Context) {
            context.stopService(Intent(context, NodeService::class.java))
        }

        /**
         * The channel, named in [context]'s language. Creating a channel
         * that exists renames it, which is the only way its name in the
         * phone's Settings ever changes — see [Notify.refreshChannels].
         */
        fun ensureChannel(context: Context) {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
            val mgr = context.getSystemService(NotificationManager::class.java) ?: return
            mgr.createNotificationChannel(
                NotificationChannel(
                    CHANNEL,
                    context.getString(R.string.nodeservice_channel_name),
                    // Low: it is a status, not an event. IMPORTANCE_MIN would
                    // hide it, which the system does not allow for a foreground
                    // service anyway.
                    NotificationManager.IMPORTANCE_LOW,
                ).apply {
                    description = context.getString(R.string.nodeservice_channel_desc)
                    setShowBadge(false)
                }
            )
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    /**
     * Where this service's words come from. A service's own context is the
     * system's, not the Application's wrapped one, so `getString` here
     * spoke the phone's language while every screen spoke the chosen one —
     * "Staying reachable" in English under an app in Arabic. Wrapped fresh
     * per command rather than once at attach, because a language change
     * recreates the activity and leaves a running service where it was.
     */
    private fun words(): Context = LocaleWrapper.wrap(this)

    override fun onCreate() {
        super.onCreate()
        ensureChannel(words())
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val words = words()
        val inCallWith = if (intent?.action == ACTION_HANG_UP) {
            Calls.hangUp()
            null
        } else {
            intent?.getStringExtra(EXTRA_IN_CALL_WITH)
        }
        // Placed but not answered: the same notification and the same type,
        // titled for what is happening — see Calls.Shell.calling for why the
        // type is taken this early.
        val ringing = intent?.getBooleanExtra(EXTRA_RINGING, false) == true
        val tap = PendingIntent.getActivity(
            this, 0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE,
        )
        val n: Notification = NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_cat_notify)
            .setContentIntent(tap)
            .setOngoing(true)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .apply {
                if (inCallWith != null) {
                    setContentTitle(
                        words.getString(
                            if (ringing) R.string.call_notify_calling else R.string.call_notify_in_call,
                            inCallWith,
                        ),
                    )
                    setContentText(words.getString(R.string.call_notify_return))
                    addAction(
                        0, words.getString(R.string.call_end_btn),
                        PendingIntent.getService(
                            this@NodeService, 1,
                            Intent(this@NodeService, NodeService::class.java).setAction(ACTION_HANG_UP),
                            PendingIntent.FLAG_IMMUTABLE,
                        ),
                    )
                } else {
                    setContentTitle(words.getString(R.string.nodeservice_notification_title))
                    setContentText(words.getString(R.string.nodeservice_notification_text))
                }
            }
            .build()

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            // specialUse, declared with its reason in the manifest. Not
            // dataSync: Android 15 allows that type six hours a day in the
            // background, then calls onTimeout and kills the process of a
            // service that does not stop — a node down at hour six.
            //
            // Plus microphone for the length of a call, and only with the
            // permission in hand — the type is refused without it, and
            // refused from the background — so a refusal falls back to the
            // plain node rather than taking the service down with it.
            val mic = inCallWith != null &&
                checkSelfPermission(android.Manifest.permission.RECORD_AUDIO) ==
                android.content.pm.PackageManager.PERMISSION_GRANTED
            val types = ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE or
                (if (mic) ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE else 0)
            try {
                startForeground(ID, n, types)
            } catch (e: Exception) {
                DucatLog.w("NodeService", "foreground type ${types}: ${e.message}")
                // The fallback can be refused too — a service told to go
                // foreground from the background is refused whatever type it
                // asks for — and this one runs inside onStartCommand, where
                // a throw is not a failed call but a dead process. The node
                // lives with the process, so that costs every conversation
                // on the phone to save a notification. Better a service the
                // system may reap later than one this line kills now.
                try {
                    startForeground(ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
                } catch (e2: Exception) {
                    DucatLog.w("NodeService", "no foreground at all: ${e2.message}")
                }
            }
        } else {
            // Same reasoning as the branch above, for the reason that
            // predates the types: Android 12 refuses a foreground start from
            // the background outright, and the refusal is an exception in
            // onStartCommand either way.
            try {
                startForeground(ID, n)
            } catch (e: Exception) {
                DucatLog.w("NodeService", "no foreground: ${e.message}")
            }
        }

        // START_STICKY: if the system kills us for memory, come back. A node
        // that stays down after pressure is a contact list that silently stops
        // working.
        return START_STICKY
    }

    /**
     * Android asking the service to stop: a foreground-service type has run
     * out its daily allowance. specialUse has none today; if a release adds
     * one, stopping is the only answer that keeps the process alive — a
     * service that ignores the ask is killed with everything in it. The node
     * lives with the process, not the service, and the next time the app is
     * looked at [DucatApplication] starts the service again.
     */
    override fun onTimeout(startId: Int, fgsType: Int) {
        DucatLog.w("NodeService", "foreground allowance spent (type $fgsType); standing down")
        stopSelf()
    }

    override fun onTimeout(startId: Int) {
        DucatLog.w("NodeService", "foreground allowance spent; standing down")
        stopSelf()
    }
}
