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
/**
 * Whether anyone is looking, app-wide.
 *
 * The poller reads this to pick its pace: every wake sweeps while a screen is
 * up; in a pocket the sweep runs when a watch rings or on a heartbeat. Counted
 * from activity starts/stops rather than any single screen, because the answer
 * is about the app, not a composable.
 */
object AppVisibility {
    @Volatile var foreground = false
        internal set
}

class DucatApplication : Application() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    // So the app context — the one background notifications and the node service
    // format their text against — speaks the user's chosen language too, not
    // only the screens the activity draws.
    override fun attachBaseContext(base: android.content.Context) {
        super.attachBaseContext(LocaleWrapper.wrap(base))
    }

    override fun onCreate() {
        super.onCreate()
        // First, so everything after it — including a crash in the next
        // line — is on the record.
        DucatLog.init(this)
        // Here as well as in MainActivity: a poller wake or a notification
        // can run in a process no activity has ever started, and until the
        // activity's assignment lands the fallback name stayed English on a
        // translated phone. attachBaseContext above has already applied the
        // chosen language; the activity's copy still refreshes it after an
        // in-app language change, which recreates the activity but not this.
        ContactNaming.unnamed = getString(R.string.contact_unnamed)
        // Started-activity count → foreground flag. Config changes pass
        // through as stop+start of different instances; the count holds.
        registerActivityLifecycleCallbacks(object : ActivityLifecycleCallbacks {
            private var started = 0
            override fun onActivityStarted(a: android.app.Activity) {
                AppVisibility.foreground = ++started > 0
            }
            override fun onActivityStopped(a: android.app.Activity) {
                AppVisibility.foreground = --started > 0
            }
            override fun onActivityCreated(a: android.app.Activity, b: android.os.Bundle?) {}
            override fun onActivityResumed(a: android.app.Activity) {}
            override fun onActivityPaused(a: android.app.Activity) {}
            override fun onActivitySaveInstanceState(a: android.app.Activity, b: android.os.Bundle) {}
            override fun onActivityDestroyed(a: android.app.Activity) {}
        })
        VeilidInit.ensure(this)
        scope.launch {
            // Migrate the sensitive stores to their encrypted form here, once,
            // off the main thread, before anything reads them. Both are lazy by
            // construction — a store migrates the first time securePrefs() names
            // it — but ducat_ceremonies is only named when a ceremony runs, so a
            // phone that upgraded and never opened another escrow would leave its
            // key shares in plaintext indefinitely. Naming both now closes that
            // window on the first post-upgrade launch. Idempotent after: the
            // _migrated flag makes each of these a cached lookup.
            runCatching {
                securePrefs(this@DucatApplication, "ducat_contacts")
                securePrefs(this@DucatApplication, "ducat_ceremonies")
            }
            // Tried and measured on the emulator: UDP-on reads but its set
            // fanout dies inside QEMU user-net; UDP-off gets zero peers at
            // all. SLIRP cannot carry a Veilid node either way, so the flag
            // stays available for future transports and real devices keep UDP.
            // Said out loud when it fails. This was a bare runCatching whose
            // result went nowhere, so a node that would not start left no
            // trace at all: the log's next line was the poller reporting a
            // transport that was never going to come up, and working out why
            // meant noticing the *absence* of lines. The poller retries (see
            // REVIVE_EVERY_MS); this is how anyone finds out it had to.
            runCatching { nodeStart("${filesDir.absolutePath}/veilid", udp = true) }
                .onFailure {
                    DucatLog.w("App", "node start: ${it.javaClass.simpleName}: ${it.message}")
                }
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
