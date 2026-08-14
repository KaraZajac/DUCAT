package org.ducatproject.ducat

import android.app.Application
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import uniffi.ducat_mobile.nodeStart

/**
 * The node starts with the app, not with a screen.
 *
 * A route takes seconds to build and a tap has three (§15.3). Starting Veilid
 * when someone opens the payment screen would put the transport's startup
 * directly into the budget the protocol spends most of its care on — and the
 * user would experience it as the payment being slow.
 *
 * Startup is off the main thread and its failure is not fatal: the rest of the
 * app works without a network, and a wallet that refuses to open because a
 * transport is down would be worse than one that cannot currently tap.
 */
class DucatApplication : Application() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    override fun onCreate() {
        super.onCreate()
        // First, so everything after it — including a crash in the next
        // line — is on the record.
        DucatLog.init(this)
        VeilidInit.ensure(this)
        scope.launch {
            runCatching { nodeStart("${filesDir.absolutePath}/veilid") }
            // Answering starts with the app, not with a screen. A contact who
            // messages while Ducat sits in the background must still be
            // answered — a peer that only replies when someone is looking at it
            // is not reachable in any sense a person would recognise.
            Poller(this@DucatApplication).start(scope)
            // Hold the process up once the node is actually running, so the
            // notification never appears in front of a node that failed to
            // start.
            runCatching { NodeService.start(this@DucatApplication) }
            // Failure is surfaced by the status panel rather than thrown here.
            // Nothing else can act on it, and crashing at launch over a
            // transport is a wallet that will not open in a tunnel.
        }
    }
}
