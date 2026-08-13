package org.ducatproject.ducat

import android.content.Context
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

private const val TAG = "DucatPoller"

/**
 * Collects what has arrived, on a timer.
 *
 * This replaces the responder that answered `app_call`s. Nothing pushes to us
 * any more, and that is the point: a message written while this phone was off
 * is waiting in a record rather than lost with a route. The cost is latency
 * bounded by the poll interval instead of arrival being instant.
 *
 * Veilid can watch a record and report changes, which would cut the interval
 * out. That is worth doing and is deliberately not done first: a poll is
 * correct whether or not a watch survives backgrounding, and correctness is
 * what this rewrite is buying.
 */
class Poller(private val context: Context) {

    /** Probe the candidates and remember the first usable one. */
    private fun pickNode(context: Context): String? = try {
        val store = NodeStore(context)
        val s = uniffi.ducat_mobile.moneroPickNode(
            uniffi.ducat_mobile.moneroDefaultNodes(store.ownUrl()),
            "stagenet",
            8_000u,
        )
        store.rememberLastGood(s.url)
        DucatLog.i(TAG, "picked node ${s.url} at height ${s.height}")
        s.url
    } catch (e: Exception) {
        DucatLog.w(TAG, "no node: ${e.message}")
        null
    }

    fun start(scope: CoroutineScope) {
        scope.launch(Dispatchers.IO) {
            while (isActive) {
                runCatching {
                    // Answers to a card we handed out land in its inbox, and
                    // that only becomes a contact once somebody looks.
                    Mailbox.collectClaims(context)
                    val n = Mailbox.poll(context)
                    if (n > 0) DucatLog.i(TAG, "collected $n message(s)")
                }.onFailure { DucatLog.w(TAG, "poll: ${it.message}") }

                // The chain, in windows. Kept on the same loop as messages
                // because both are "what happened while we were not looking",
                // and a second timer would just be another thing to get wrong.
                runCatching {
                    // Find a node ourselves rather than waiting for a screen to
                    // do it. This used to read whatever the Status panel had
                    // last stored, which meant the wallet did not sync until the
                    // user happened to open that screen — a background job that
                    // depends on a screen having run is not a background job.
                    val node = NodeStore(context).lastGood() ?: pickNode(context)
                    if (node != null) {
                        val moved = Wallet.scanStep(context, node)
                        if (moved) Wallet.refreshSpent(context, node)
                    }
                }.onFailure { DucatLog.w(TAG, "scan: ${it.message}") }

                // Cheap: it only leaves the device when the cache has expired.
                runCatching { Rates.refresh(context) }
                    .onFailure { DucatLog.w(TAG, "rate: ${it.message}") }

                delay(10_000)
            }
        }
    }
}
