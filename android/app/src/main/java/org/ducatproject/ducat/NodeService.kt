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

        fun start(context: Context) {
            val i = Intent(context, NodeService::class.java)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(i)
            } else {
                context.startService(i)
            }
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, NodeService::class.java))
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val mgr = getSystemService(NotificationManager::class.java)
            mgr.createNotificationChannel(
                NotificationChannel(
                    CHANNEL,
                    "Staying reachable",
                    // Low: it is a status, not an event. IMPORTANCE_MIN would
                    // hide it, which the system does not allow for a foreground
                    // service anyway.
                    NotificationManager.IMPORTANCE_LOW,
                ).apply {
                    description = "Keeps DUCAT reachable so contacts can pay you and message you."
                    setShowBadge(false)
                }
            )
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val tap = PendingIntent.getActivity(
            this, 0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE,
        )
        val n: Notification = NotificationCompat.Builder(this, CHANNEL)
            .setContentTitle("DUCAT is reachable")
            .setContentText("Contacts can reach you while this is running.")
            .setSmallIcon(R.drawable.ic_ducat_mono)
            .setContentIntent(tap)
            .setOngoing(true)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .build()

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            startForeground(ID, n)
        }

        // START_STICKY: if the system kills us for memory, come back. A node
        // that stays down after pressure is a contact list that silently stops
        // working.
        return START_STICKY
    }
}
