package org.ducatproject.ducat

import android.content.Context
import java.io.File
import uniffi.ducat_mobile.ListingDoc

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
 * What arrives is a *bundle*: pictures, and since 1.1 possibly a
 * `listing.json` beside them naming the pictures, the seller's description,
 * attached files and the specs the notice does not carry. [bundle] reads
 * it the way the section says to — a document that will not open is
 * refused whole and the pictures shown alone.
 *
 * Keyed by digest rather than by listing: the digest *is* the content, so
 * two listings carrying the same pictures share one directory and a
 * re-post of unchanged photographs finds them already here.
 */
object Galleries {
    private const val TAG = "Galleries"

    /** The document's name at the root of the share (§16.18.3). */
    private const val DOC = "listing.json"

    /** What one gallery's fetch is doing, for the screen. */
    data class State(
        val dir: File?,
        val fetching: Boolean,
        val progress: Swarm.Progress?,
        val failed: String?,
    )

    /** One picture, with the caption its document gave it — none, for a
     *  bundle without one. */
    data class Picture(val file: File, val caption: String)

    /** One attached file, under the name and type the document promised. */
    data class Attachment(val file: File, val name: String, val mime: String)

    /**
     * What a fetched bundle holds, as a screen wants it: the document when
     * there is one that opens, the pictures, the files.
     */
    data class Bundle(val doc: ListingDoc?, val pictures: List<Picture>, val files: List<Attachment>)

    private val lock = Any()
    private val running = HashSet<String>()
    private val failures = HashMap<String, String>()

    fun dirFor(context: Context, digestHex: String): File =
        File(File(context.filesDir, "listing_galleries"), safe(digestHex))

    /** Where a fetch lands until it is whole; see [start]. */
    private fun partFor(context: Context, digestHex: String): File =
        File(File(context.filesDir, "listing_galleries"), safe(digestHex) + ".part")

    private fun safe(hex: String): String =
        hex.filter { it.isLetterOrDigit() }.take(64).ifBlank { "unnamed" }

    /**
     * What arrived for that digest, read the way §16.18.3 says to.
     *
     * A `listing.json` at the root names the pictures and the files. A
     * bundle without one is the picture-only gallery the section began
     * with, and every image in it is shown. A document that will not open
     * is refused whole — logged, never shown in part — and the pictures
     * shown alone, because a bad document must not hide good pictures.
     * Null before the fetch has landed.
     */
    fun bundle(context: Context, digestHex: String): Bundle? {
        val dir = dirFor(context, digestHex)
        if (!dir.isDirectory) return null
        val docFile = File(dir, DOC)
        val doc = if (docFile.isFile) {
            runCatching { uniffi.ducat_mobile.listingDocParse(docFile.readText()) }
                .onFailure {
                    DucatLog.w(
                        TAG,
                        "gallery ${digestHex.take(12)}…: listing.json refused, " +
                            "showing the pictures alone: ${it.message}",
                    )
                }
                .getOrNull()
        } else {
            null
        }
        val pictures = if (doc != null) {
            doc.pictures.mapNotNull { p -> file(context, digestHex, p.path)?.let { Picture(it, p.caption) } }
        } else {
            imageFiles(dir).map { Picture(it, "") }
        }
        val files = doc?.files.orEmpty().mapNotNull { f ->
            file(context, digestHex, f.path)?.let { Attachment(it, f.name, f.mime) }
        }
        return Bundle(doc, pictures, files)
    }

    /**
     * A file inside a fetched bundle, by its bundle path — and only inside
     * it. The document's paths were checked when it was parsed, but the
     * description's image targets reach here too, and this is the boundary
     * a bad one would cross: canonicalised and compared, the way a home's
     * bundle does it.
     */
    fun file(context: Context, digestHex: String, rel: String): File? {
        if (rel.contains("..") || rel.contains(':') || rel.startsWith("//")) return null
        val root = dirFor(context, digestHex)
        val f = File(root, rel.trimStart('/'))
        val rootCanon = root.canonicalPath + File.separator
        return f.takeIf { it.isFile && it.canonicalPath.startsWith(rootCanon) }
    }

    /** Every image in the bundle, wherever it sits, in path order: the
     *  reading for a bundle with no document, which is every bundle written
     *  before there was one. */
    private fun imageFiles(dir: File): List<File> =
        dir.walkTopDown()
            .filter { it.isFile && it.extension.lowercase() in IMAGE_EXTENSIONS }
            .sortedBy { it.path }
            .toList()

    private val IMAGE_EXTENSIONS = setOf("jpg", "jpeg", "png", "webp", "gif")

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
     * Into a sibling until it is whole, then into place, as a release
     * does. Fetching straight into the directory the screen reads meant a
     * fetch that died half-way left files there, and "some files" is what
     * [state] calls having the gallery: the listing showed three pictures
     * for ever and never asked for the rest.
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
            val part = partFor(app, digestHex)
            try {
                part.deleteRecursively()
                part.mkdirs()
                Swarm.fetch(share, digestHex, part.absolutePath)
                dir.deleteRecursively()
                check(part.renameTo(dir)) { "could not move the gallery into place" }
                DucatLog.i(TAG, "gallery ${digestHex.take(12)}… fetched")
            } catch (e: Throwable) {
                // Named, not swallowed: a gallery that never arrives looks
                // exactly like one still arriving, and the difference is
                // the only thing the reader can act on.
                DucatLog.w(TAG, "gallery ${digestHex.take(12)}…: ${e.message}")
                synchronized(lock) {
                    failures[share] = e.saidWhy() ?: e.javaClass.simpleName
                }
            } finally {
                synchronized(lock) { running.remove(share) }
            }
        }.apply { isDaemon = true; name = "gallery-fetch" }.start()
    }
}
