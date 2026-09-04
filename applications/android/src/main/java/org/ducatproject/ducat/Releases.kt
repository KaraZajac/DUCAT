package org.ducatproject.ducat

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/**
 * A file put on the network once, at an address that cannot change.
 *
 * The third shape in the Library, beside §16.20's paid periods: a record
 * of a CD, a film, a dataset — something shared rather than sold, and
 * finished the moment it leaves. Where a §16.22 site is one mutable head
 * at a stable key, and a publication is a stream of periods behind a
 * paywall, a release is a single fixed thing and nothing more.
 *
 * **Immutability is arithmetic here, not policy.** The address carries the
 * swarm share key *and* the content digest, so changing a byte changes the
 * address: there is no version of this object that can be updated, not
 * even by whoever made it, and no head for a mirror to chase. That is why
 * a release needs none of the machinery Sites.reseed does — a mirror
 * announces the pair it holds, for as long as it holds it, and can never
 * be announcing the wrong edition of anything.
 *
 * The honest cost, which the screen must say rather than imply away:
 * nobody is *obliged* to serve a release. It lives exactly as long as
 * somebody keeps it alive, and when the last mirror drops it the address
 * still parses and no longer resolves.
 */
object Releases {
    private const val PREFIX = "ducat:file/"
    private val lock = Any()

    private fun prefs(context: Context) = securePrefs(context, "ducat_releases")

    data class Release(
        val shareKey: String,
        val digestHex: String,
        /** The publisher's own name for it; display only, never a path. */
        val title: String,
        val addedAt: Long,
        val bytes: Long,
        /** Kept alive for other readers — the same promise a site's
         *  checkbox makes, and the only thing keeping this address alive
         *  once whoever shared it has gone. */
        val keepAlive: Boolean,
        /** True when this device is where it came from. Not a claim of
         *  authorship: a release has no owner and no write authority, only
         *  a first seeder. */
        val mine: Boolean,
    )

    /**
     * `ducat:file/<share-key>:<digest-hex>` — the whole of the address.
     *
     * The digest rides in it deliberately. A reader who has the address can
     * verify what they were given against what they asked for, without
     * trusting whoever handed it over; and it is what makes the object
     * immutable, since there is no way to name "the newer one".
     */
    fun uriOf(shareKey: String, digestHex: String): String = "$PREFIX$shareKey:$digestHex"

    /**
     * The pair back out of an address, or null.
     *
     * Share keys carry colons of their own (`VLD0:<key>:<secret>`), so the
     * digest is taken from the LAST colon and the key is everything before
     * it — splitting on the first would hand back a truncated key that
     * looks plausible and resolves to nothing.
     */
    fun parse(uri: String): Pair<String, String>? {
        val body = uri.trim().removePrefix(PREFIX).takeIf { it != uri.trim() } ?: return null
        val cut = body.lastIndexOf(':')
        if (cut <= 0 || cut == body.length - 1) return null
        val key = body.substring(0, cut)
        val digest = body.substring(cut + 1)
        if (key.isBlank() || digest.length != 64 || digest.any { it !in "0123456789abcdefABCDEF" }) {
            return null
        }
        return key to digest.lowercase()
    }

    fun all(context: Context): List<Release> {
        val raw = prefs(context).getString("releases", null) ?: return emptyList()
        val arr = runCatching { JSONArray(raw) }.getOrNull() ?: return emptyList()
        return (0 until arr.length()).mapNotNull { i ->
            runCatching {
                val o = arr.getJSONObject(i)
                Release(
                    shareKey = o.getString("key"),
                    digestHex = o.getString("digest"),
                    title = o.optString("title"),
                    addedAt = o.optLong("added"),
                    bytes = o.optLong("bytes"),
                    keepAlive = o.optBoolean("keep", false),
                    mine = o.optBoolean("mine", false),
                )
            }.getOrNull()
        }
    }

    private fun save(context: Context, items: List<Release>) {
        val arr = JSONArray()
        for (r in items) {
            arr.put(
                JSONObject()
                    .put("key", r.shareKey).put("digest", r.digestHex)
                    .put("title", r.title).put("added", r.addedAt)
                    .put("bytes", r.bytes).put("keep", r.keepAlive).put("mine", r.mine),
            )
        }
        prefs(context).edit().putString("releases", arr.toString()).apply()
        ContactStore.bump()
    }

    /** Replaced where it stands, appended only when new — a row that moves
     *  under the hand about to tap it is the fault Listings and Sites both
     *  had. Keyed by digest, since that is the identity. */
    private fun put(context: Context, r: Release) = synchronized(lock) {
        val cur = all(context)
        save(
            context,
            if (cur.none { it.digestHex == r.digestHex }) {
                cur + r
            } else {
                cur.map { if (it.digestHex == r.digestHex) r else it }
            },
        )
    }

    /** Where a release's bytes live. Named by the digest, because that is
     *  what the address promises and what a second copy of the same file
     *  would hash to anyway. */
    fun dirFor(context: Context, digestHex: String): java.io.File =
        java.io.File(context.filesDir, "releases/$digestHex")

    fun isHere(context: Context, digestHex: String): Boolean =
        dirFor(context, digestHex).let { it.isDirectory && it.walkTopDown().any { f -> f.isFile } }

    /**
     * Put a file on the network and keep serving it.
     *
     * Seeds from a directory holding one file, so the swarm index carries a
     * name the far end can write to disk. Returns the release, whose
     * address is the only thing that needs handing over.
     */
    fun share(context: Context, source: java.io.File, title: String): Release {
        val staging = java.io.File(context.filesDir, "release_staging").apply {
            deleteRecursively(); mkdirs()
        }
        val name = java.io.File(source.name).name.takeIf { it.isNotBlank() } ?: "file"
        source.copyTo(java.io.File(staging, name), overwrite = true)
        val share = Swarm.seed(staging.absolutePath)
        val dir = dirFor(context, share.indexDigestHex)
        dir.deleteRecursively()
        dir.parentFile?.mkdirs()
        check(staging.renameTo(dir)) { "could not move the release into place" }
        // Seeded from the staging path, which has just moved: park it again
        // from where it will stay, or the seeder serves a directory that is
        // no longer there.
        Swarm.stopShare(share.shareKey)
        val r = Release(
            shareKey = share.shareKey,
            digestHex = share.indexDigestHex,
            title = title.ifBlank { name },
            addedAt = System.currentTimeMillis() / 1000,
            bytes = dir.walkTopDown().filter { it.isFile }.sumOf { it.length() },
            keepAlive = true,
            mine = true,
        )
        put(context, r)
        reseed(context, r.digestHex)
        DucatLog.i("Releases", "shared '${r.title}' at ${uriOf(r.shareKey, r.digestHex)}")
        return r
    }

    /** File an address somebody handed over. Nothing is fetched yet. */
    fun add(context: Context, uri: String, title: String = ""): Release? {
        val (key, digest) = parse(uri) ?: return null
        val prior = all(context).firstOrNull { it.digestHex == digest }
        val r = Release(
            shareKey = key,
            digestHex = digest,
            title = prior?.title?.ifBlank { null } ?: title,
            addedAt = prior?.addedAt ?: (System.currentTimeMillis() / 1000),
            bytes = prior?.bytes ?: 0L,
            keepAlive = prior?.keepAlive ?: false,
            mine = prior?.mine ?: false,
        )
        put(context, r)
        return r
    }

    fun setKeepAlive(context: Context, digestHex: String, keep: Boolean) {
        synchronized(lock) {
            save(context, all(context).map { if (it.digestHex == digestHex) it.copy(keepAlive = keep) else it })
        }
        if (keep) reseed(context, digestHex) else {
            all(context).firstOrNull { it.digestHex == digestHex }
                ?.let { runCatching { Swarm.stopShare(it.shareKey) } }
        }
    }

    fun remove(context: Context, digestHex: String) {
        all(context).firstOrNull { it.digestHex == digestHex }?.let {
            runCatching { Swarm.stopShare(it.shareKey) }
        }
        synchronized(lock) { save(context, all(context).filterNot { it.digestHex == digestHex }) }
        dirFor(context, digestHex).deleteRecursively()
    }

    /** Fetch it, verifying every piece against the digest in the address.
     *  Stays seeding when the reader chose to keep it alive. */
    fun fetch(context: Context, r: Release): java.io.File {
        val dir = dirFor(context, r.digestHex)
        if (isHere(context, r.digestHex)) return dir
        val part = java.io.File(dir.parentFile, "${r.digestHex}.part")
        part.deleteRecursively(); part.mkdirs()
        Swarm.fetch(r.shareKey, r.digestHex, part.absolutePath)
        dir.deleteRecursively()
        check(part.renameTo(dir)) { "could not move the release into place" }
        put(
            context,
            r.copy(bytes = dir.walkTopDown().filter { it.isFile }.sumOf { it.length() }),
        )
        if (all(context).firstOrNull { it.digestHex == r.digestHex }?.keepAlive == true) {
            reseed(context, r.digestHex)
        }
        return dir
    }

    /**
     * Serve what we hold.
     *
     * No head to re-read, unlike a site: the address names one fixed thing,
     * so the pair on disk is the only pair there has ever been for this
     * release and a mirror can announce it without asking anybody.
     */
    fun reseed(context: Context, digestHex: String) {
        val r = all(context).firstOrNull { it.digestHex == digestHex } ?: return
        if (!r.keepAlive) return
        val dir = dirFor(context, digestHex)
        if (!isHere(context, digestHex)) {
            DucatLog.i("Releases", "'${r.title}' is not on this device yet — nothing to serve")
            return
        }
        Thread {
            runCatching {
                if (all(context).none { it.digestHex == digestHex && it.keepAlive }) {
                    return@runCatching
                }
                Swarm.stopShare(r.shareKey)
                Swarm.fetch(r.shareKey, r.digestHex, dir.absolutePath, staySeeding = true)
                if (all(context).none { it.digestHex == digestHex && it.keepAlive }) {
                    Swarm.stopShare(r.shareKey)
                    DucatLog.i("Releases", "reseed finished for a release no longer kept — stopped")
                }
            }.onFailure { DucatLog.w("Releases", "reseed '${r.title}': ${it.message}") }
        }.apply { isDaemon = true; name = "release-reseed" }.start()
    }

    /** Drop directories no saved release claims. */
    fun sweepOrphans(context: Context): Int {
        val root = java.io.File(context.filesDir, "releases")
        if (!root.isDirectory) return 0
        val keep = all(context).mapTo(HashSet()) { it.digestHex }
        val gone = root.listFiles()?.filter { it.isDirectory && it.name.removeSuffix(".part") !in keep }
            ?: return 0
        for (d in gone) d.deleteRecursively()
        if (gone.isNotEmpty()) DucatLog.i("Releases", "swept ${gone.size} orphaned release(s)")
        return gone.size
    }
}
