package org.ducatproject.ducat

import android.content.Context
import java.io.File

/**
 * §16.18.3's gallery, on the reader's side.
 *
 * A listing's thumbnail rides the board; its full-size photographs do not.
 * They sit on a swarm share whose key and digest the notice carries, and
 * this fetches them — **only for a listing somebody opened**. The spec is
 * explicit that a client must not do this while browsing: peer discovery
 * costs tens of seconds before a first piece moves, and a browse screen
 * starting eight of them has turned a board read into a stall.
 *
 * Two things a reader should understand, and the screen says both. The
 * pictures come from the seller's own device, so a seller who is away has
 * a gallery that does not arrive — the thumbnail still does, which is the
 * whole argument for putting it on the board. And fetching is a peer
 * connection to that device, so opening the photographs tells the seller
 * somebody is looking, where reading the board tells them nothing.
 *
 * Keyed by digest rather than by listing: the digest *is* the content, so
 * two listings carrying the same pictures share one directory and a
 * re-post of unchanged photographs finds them already here.
 */
object Galleries {

    /** What one gallery's fetch is doing, for the screen. */
    data class State(
        val dir: File?,
        val fetching: Boolean,
        val progress: Swarm.Progress?,
        val failed: String?,
    )

    private val lock = Any()
    private val running = HashSet<String>()
    private val failures = HashMap<String, String>()

    fun dirFor(context: Context, digestHex: String): File =
        File(File(context.filesDir, "listing_galleries"), safe(digestHex))

    private fun safe(hex: String): String =
        hex.filter { it.isLetterOrDigit() }.take(64).ifBlank { "unnamed" }

    /** Every picture already on this phone for that digest, in name order. */
    fun photos(context: Context, digestHex: String): List<File> =
        dirFor(context, digestHex).listFiles()?.filter { it.isFile }?.sortedBy { it.name }
            ?: emptyList()

    fun state(context: Context, share: String?, digestHex: String?): State {
        if (share.isNullOrBlank() || digestHex.isNullOrBlank()) {
            return State(null, false, null, null)
        }
        val dir = dirFor(context, digestHex)
        val have = dir.isDirectory && dir.walkTopDown().any { it.isFile }
        val busy = synchronized(lock) { share in running }
        return State(
            dir = if (have) dir else null,
            fetching = busy,
            progress = if (busy) runCatching { Swarm.fetchProgress(share) }.getOrNull() else null,
            failed = synchronized(lock) { failures[share] },
        )
    }

    /**
     * Fetch, once, on a thread of its own.
     *
     * A daemon thread and the application context, the way LibraryFetch
     * does it: the screen that started this can be gone by the time the
     * first piece lands, and a fetch that died with its sheet would leave a
     * half-written directory nobody finishes.
     *
     * Not staySeeding. A reader who looked at a bicycle has not volunteered
     * to serve its photographs to strangers, and §16.18.3 asks nobody to.
     */
    fun start(context: Context, share: String, digestHex: String) {
        val app = context.applicationContext
        synchronized(lock) {
            if (share in running) return
            running.add(share)
            failures.remove(share)
        }
        Thread {
            val dir = dirFor(app, digestHex)
            try {
                dir.mkdirs()
                Swarm.fetch(share, digestHex, dir.absolutePath)
                DucatLog.i("Galleries", "gallery ${digestHex.take(12)}… fetched")
            } catch (e: Throwable) {
                // Named, not swallowed: a gallery that never arrives looks
                // exactly like one still arriving, and the difference is
                // the only thing the reader can act on.
                DucatLog.w("Galleries", "gallery ${digestHex.take(12)}…: ${e.message}")
                synchronized(lock) {
                    failures[share] = e.saidWhy() ?: e.javaClass.simpleName
                }
            } finally {
                synchronized(lock) { running.remove(share) }
            }
        }.apply { isDaemon = true; name = "gallery-fetch" }.start()
    }
}
