package org.ducatproject.ducat

import android.content.Context
import android.graphics.BitmapFactory
import android.net.Uri
import java.io.File
import uniffi.ducat_mobile.FeedDoc
import uniffi.ducat_mobile.FeedEntry
import uniffi.ducat_mobile.FeedFile
import uniffi.ducat_mobile.FeedMedia
import uniffi.ducat_mobile.FeedPost

/**
 * §16.23 on the phone: a persona's home — its site at the address its own
 * key names — and the feed inside it; hearts, and the timeline of the
 * people kept. The rules live in the shared crate (feed parsing, the text
 * subset, the merge, the pages); this object keeps the files and drives
 * the site machinery, the same way the desk's home module does.
 */
object Home {
    private const val TAG = "Home"
    private const val SUBKEYS = 1u
    /** A thumbnail's ceiling in the bundle; the picture itself travels as a share. */
    private const val THUMB_BUDGET = 160 * 1024
    private const val FEEDS_EVERY_MS = 10L * 60 * 1000
    private const val PAGE_KEEP = 120
    private const val MAX_POSTS = 200

    @Volatile private var lastFeeds = 0L

    fun keyOf(personaHex: String): String {
        val pk = hexToBytes(personaHex)?.takeIf { it.size == 32 } ?: throw IllegalArgumentException("not a persona key")
        return uniffi.ducat_mobile.nodeDhtRecordKeyFor(pk, SUBKEYS)
    }

    fun myKey(context: Context): String = keyOf(PersonaStore(context).worn())

    private fun dir(context: Context, hex: String): File = File(context.filesDir, "home/$hex").apply { mkdirs() }

    /** The persona's own pages, if it keeps any beside the feed. */
    fun pagesDir(context: Context): File = File(dir(context, PersonaStore(context).worn()), "site")

    /** The worn persona's home record, created here if need be, registered as a site of mine. */
    fun ensureHome(context: Context): Sites.Site {
        val personas = PersonaStore(context)
        val worn = personas.worn()
        val key = keyOf(worn)
        Sites.all(context).firstOrNull { it.recordKey == key && it.mine }?.let { return it }
        val secret = personas.secretFor(worn) ?: personas.secret()
        val rec = uniffi.ducat_mobile.nodeDhtCreateOwned(SUBKEYS, hexToBytes(worn)!!, secret)
        require(rec.key == key) { "the home record came back at ${rec.key} not $key" }
        val prior = Sites.all(context).firstOrNull { it.recordKey == key }
        val now = System.currentTimeMillis() / 1000
        val entry = Sites.Site(
            recordKey = key,
            title = NameStore(context, worn).get() ?: "",
            share = prior?.share.orEmpty(),
            digestHex = prior?.digestHex.orEmpty(),
            updated = prior?.updated ?: 0L,
            addedAt = prior?.addedAt ?: now,
            keepAlive = true,
            fetchedDigestHex = prior?.fetchedDigestHex,
            fetchedShare = prior?.fetchedShare,
            ownerPublic = rec.ownerPublic,
            ownerSecret = rec.ownerSecret,
        )
        Sites.upsert(context, entry)
        DucatLog.i(TAG, "home record ready at ${key.take(24)}…")
        return entry
    }

    // ----- my feed -------------------------------------------------------------------

    private fun feedFile(context: Context, hex: String) = File(dir(context, hex), "feed.json")

    fun myFeed(context: Context): FeedDoc {
        val worn = PersonaStore(context).worn()
        val f = feedFile(context, worn)
        if (f.isFile) return uniffi.ducat_mobile.feedParse(f.readText())
        return FeedDoc(v = 1uL, persona = worn, name = NameStore(context, worn).get() ?: "", updated = 0uL, posts = emptyList(), older = null)
    }

    private fun saveMyFeed(context: Context, doc: FeedDoc) {
        feedFile(context, doc.persona).writeText(uniffi.ducat_mobile.feedEncode(doc))
    }

    /** Both answered by Releases, which the listing bundle (§16.18.3)
     *  shares with the desk; the feed asks the same two questions. */
    private fun mimeOf(name: String): String = Releases.mimeOf(name)

    private fun nameOf(context: Context, uri: Uri): String = Releases.nameOf(context, uri)

    /** A picked thing copied under its own name, so the share carries that name. */
    private fun copyIn(context: Context, uri: Uri, staging: File): File {
        val name = nameOf(context, uri).replace('/', '_')
        val out = File(staging, name)
        context.contentResolver.openInputStream(uri)?.use { i -> out.outputStream().use { o -> i.copyTo(o) } }
            ?: throw IllegalStateException("could not read what was picked")
        return out
    }

    /**
     * Write a post: thumbnails into the bundle, the pictures and files
     * themselves as shares, the post at the top of the feed — then the
     * home republished so the network has it.
     */
    fun post(context: Context, text: String, media: List<Uri>, files: List<Uri>): FeedPost = try {
        postNow(context, text, media, files)
    } finally {
        // Each phase is said as it starts (Busy, below); cleared however
        // this ends, or the last phrase would stand under the next button.
        Busy.clear()
    }

    private fun postNow(context: Context, text: String, media: List<Uri>, files: List<Uri>): FeedPost {
        val body = text.trim()
        require(body.isNotEmpty() || media.isNotEmpty() || files.isNotEmpty()) { "a post needs some words, a picture, or a file" }
        require(body.length <= 4000) { "a post is at most 4000 characters" }
        require(media.size <= 8 && files.size <= 8) { "too many pictures or files for one post" }
        val worn = PersonaStore(context).worn()
        var doc = myFeed(context)
        val id = uniffi.ducat_mobile.feedNewId()
        val thumbDir = File(dir(context, worn), "feed/$id").apply { mkdirs() }
        val staging = File(context.cacheDir, "post_staging/$id").apply { deleteRecursively(); mkdirs() }
        val mediaOut = ArrayList<FeedMedia>()
        val filesOut = ArrayList<FeedFile>()
        // Every picture and file is a share of its own before the post
        // names it — a route allocation apiece, which is the slow half of
        // a post with photographs in it.
        if (media.isNotEmpty()) Busy.say(context.getString(R.string.busy_pictures_swarm))
        media.forEachIndexed { n, uri ->
            val thumb = SafeImage.thumbnail({ context.contentResolver.openInputStream(uri) }, THUMB_BUDGET)
            val copy = copyIn(context, uri, staging)
            if (thumb != null) {
                val rel = "feed/$id/${n + 1}.jpg"
                File(thumbDir, "${n + 1}.jpg").writeBytes(thumb)
                val o = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                BitmapFactory.decodeByteArray(thumb, 0, thumb.size, o)
                // The picture itself, as a share, unless the thumbnail already is the picture.
                val full = if (copy.length() > thumb.size + 8 * 1024) {
                    val r = Releases.share(context, copy, copy.name)
                    Releases.uriOf(r.shareKey, r.digestHex)
                } else null
                mediaOut.add(FeedMedia(path = rel, full = full, mime = "image/jpeg", bytes = thumb.size.toULong(), w = o.outWidth.coerceAtLeast(0).toUInt(), h = o.outHeight.coerceAtLeast(0).toUInt(), alt = ""))
            } else {
                val r = Releases.share(context, copy, copy.name)
                filesOut.add(FeedFile(name = copy.name, addr = Releases.uriOf(r.shareKey, r.digestHex), mime = mimeOf(copy.name), bytes = copy.length().toULong()))
            }
        }
        if (files.isNotEmpty()) Busy.say(context.getString(R.string.busy_files_swarm))
        for (uri in files) {
            val copy = copyIn(context, uri, staging)
            val r = Releases.share(context, copy, copy.name)
            filesOut.add(FeedFile(name = copy.name, addr = Releases.uriOf(r.shareKey, r.digestHex), mime = mimeOf(copy.name), bytes = copy.length().toULong()))
        }
        staging.deleteRecursively()
        val post = FeedPost(id = id, at = (System.currentTimeMillis() / 1000).toULong(), edited = null, text = body, media = mediaOut, files = filesOut, re = null)
        doc = doc.copy(name = NameStore(context, worn).get() ?: doc.name, updated = (System.currentTimeMillis() / 1000).toULong(), posts = listOf(post) + doc.posts)
        doc = rollOlder(context, worn, doc)
        saveMyFeed(context, doc)
        publishHome(context)
        DucatLog.i(TAG, "posted $id (${mediaOut.size} picture(s), ${filesOut.size} file(s))")
        return post
    }

    fun deletePost(context: Context, id: String) {
        val worn = PersonaStore(context).worn()
        val doc = myFeed(context)
        val kept = doc.posts.filter { it.id != id }
        require(kept.size != doc.posts.size) { "no such post" }
        saveMyFeed(context, doc.copy(posts = kept, updated = (System.currentTimeMillis() / 1000).toULong()))
        File(dir(context, worn), "feed/$id").deleteRecursively()
        publishHome(context)
    }

    /** Keep feed.json to a page; the oldest posts move to feed-<n>.json, chained by `older`. */
    private fun rollOlder(context: Context, worn: String, doc: FeedDoc): FeedDoc {
        if (doc.posts.size <= MAX_POSTS) return doc
        val d = dir(context, worn)
        var n = 1
        while (File(d, "feed-$n.json").exists()) n++
        val older = doc.posts.drop(PAGE_KEEP)
        val page = FeedDoc(v = 1uL, persona = doc.persona, name = doc.name, updated = doc.updated, posts = older, older = doc.older)
        File(d, "feed-$n.json").writeText(uniffi.ducat_mobile.feedEncode(page))
        return doc.copy(posts = doc.posts.take(PAGE_KEEP), older = "feed-$n.json")
    }

    /** Put the home on the network: pages if any, feed.json, thumbnails, older pages, a page per post. */
    fun publishHome(context: Context): Sites.Site = try {
        Busy.say(context.getString(R.string.feed_publishing_home))
        publishHomeNow(context)
    } finally {
        Busy.clear()
    }

    private fun publishHomeNow(context: Context): Sites.Site {
        val worn = PersonaStore(context).worn()
        val home = ensureHome(context)
        val doc = myFeed(context)
        val d = dir(context, worn)
        val staging = File(d, "staging").apply { deleteRecursively(); mkdirs() }
        val pages = File(d, "site")
        if (File(pages, "index.html").isFile) pages.copyRecursively(staging, overwrite = true)
        val name = doc.name.ifBlank { NameStore(context, worn).get() ?: "" }
        File(staging, "feed.json").writeText(uniffi.ducat_mobile.feedEncode(doc))
        File(d, "feed").takeIf { it.isDirectory }?.copyRecursively(File(staging, "feed"), overwrite = true)
        File(staging, "feed.html").writeText(uniffi.ducat_mobile.feedIndexHtml(doc))
        File(staging, "posts").mkdirs()
        val all = ArrayList(doc.posts)
        var older = doc.older
        while (older != null) {
            val f = File(d, older)
            if (!f.isFile) break
            f.copyTo(File(staging, older), overwrite = true)
            val page = runCatching { uniffi.ducat_mobile.feedParse(f.readText()) }.getOrNull() ?: break
            all.addAll(page.posts)
            older = page.older
        }
        for (p in all) File(staging, "posts/${p.id}.html").writeText(uniffi.ducat_mobile.feedPostHtml(name, p))
        if (!File(staging, "index.html").isFile) File(staging, "index.html").writeText(uniffi.ducat_mobile.feedIndexHtml(doc))
        val site = Sites.publish(context, staging, name.ifBlank { "Home" }, recordKey = home.recordKey)
        staging.deleteRecursively()
        return site
    }

    // ----- hearts and the timeline ---------------------------------------------------

    fun setHeart(context: Context, personaHex: String, on: Boolean) {
        val store = ContactStore(context)
        val c = store.all().firstOrNull { it.personaHex == personaHex } ?: throw IllegalArgumentException("no such contact")
        store.update(c.copy(hearted = on))
        val key = keyOf(personaHex)
        if (on) {
            runCatching {
                val site = Sites.add(context, key)
                Sites.setKeepAlive(context, key, true)
                Sites.fetchBundle(context, site)
            }.onFailure { DucatLog.i(TAG, "hearted a persona whose home is not there yet: ${it.message}") }
        } else if (Sites.all(context).any { it.recordKey == key && !it.mine }) {
            Sites.setKeepAlive(context, key, false)
        }
    }

    fun hearted(context: Context): List<Contact> = ContactStore(context).all().filter { it.hearted }

    /** Read every hearted home's head; fetch what moved. Returns how many have a new edition. */
    fun refreshFeeds(context: Context): Int {
        // Heads FEED_WIDTH side by side: one read is a second or ten
        // depending on the network, and a timeline of a few hundred homes
        // must not take the whole of its ten-minute turn on reads alone.
        var fresh = 0
        for (chunk in hearted(context).chunked(FEED_WIDTH)) {
            val tasks = chunk.map { c -> java.util.concurrent.Callable<Boolean> { refreshOneFeed(context, c) } }
            for (f in feedPool.invokeAll(tasks)) if (runCatching { f.get() }.getOrDefault(false)) fresh++
        }
        return fresh
    }

    /** One hearted home: its head, and its bundle when the head moved. True when there is a new edition on disk. */
    private fun refreshOneFeed(context: Context, c: Contact): Boolean {
        val key = runCatching { keyOf(c.personaHex) }.getOrNull() ?: return false
        val before = Sites.all(context).firstOrNull { it.recordKey == key }?.fetchedDigestHex
        val site = runCatching { Sites.add(context, key) }.getOrNull() ?: return false
        if (before == site.digestHex) return false
        return runCatching { Sites.fetchBundle(context, site) }
            .onSuccess { DucatLog.i(TAG, "${c.displayName()} has a new edition") }
            .onFailure { DucatLog.w(TAG, "${c.displayName()}'s home: ${it.message}") }
            .isSuccess
    }

    private const val FEED_WIDTH = 4
    private val feedPool: java.util.concurrent.ExecutorService by lazy {
        java.util.concurrent.Executors.newFixedThreadPool(FEED_WIDTH) { r ->
            Thread(r, "feed-heads").apply { isDaemon = true }
        }
    }

    /** Not every sweep: heads move rarely and the reads add up. */
    fun feedsTick(context: Context) {
        val now = System.currentTimeMillis()
        if (now - lastFeeds < FEEDS_EVERY_MS) return
        lastFeeds = now
        if (hearted(context).isEmpty()) return
        val n = refreshFeeds(context)
        if (n > 0) DucatLog.i(TAG, "$n home(s) have new posts")
    }

    fun feedOf(context: Context, personaHex: String): FeedDoc? {
        if (personaHex == PersonaStore(context).worn()) return runCatching { myFeed(context) }.getOrNull()
        val key = runCatching { keyOf(personaHex) }.getOrNull() ?: return null
        val f = File(Sites.bundleDir(context, key), "feed.json")
        if (!f.isFile) return null
        return runCatching { uniffi.ducat_mobile.feedParse(f.readText()) }
            .onFailure { DucatLog.w(TAG, "${personaHex.take(8)}'s feed is unreadable: ${it.message}") }
            .getOrNull()
    }

    /** Everyone kept, and me, newest first. */
    fun timeline(context: Context, limit: Int = 200): List<FeedEntry> {
        val docs = ArrayList<FeedDoc>()
        runCatching { myFeed(context) }.getOrNull()?.takeIf { it.posts.isNotEmpty() }?.let { docs.add(it) }
        for (c in hearted(context)) feedOf(context, c.personaHex)?.let { docs.add(it) }
        return uniffi.ducat_mobile.feedMerge(docs, limit.toUInt())
    }

    /** A file from a persona's home bundle, for the timeline's thumbnails. */
    fun homeFile(context: Context, personaHex: String, rel: String): File? {
        if (rel.contains("..") || rel.contains(':') || rel.startsWith("//")) return null
        val key = runCatching { keyOf(personaHex) }.getOrNull() ?: return null
        val root = if (personaHex == PersonaStore(context).worn() && File(dir(context, personaHex), rel.trimStart('/')).isFile) dir(context, personaHex) else Sites.bundleDir(context, key)
        val f = File(root, rel.trimStart('/'))
        val rootCanon = root.canonicalPath + File.separator
        return f.takeIf { it.isFile && it.canonicalPath.startsWith(rootCanon) }
    }

    fun myHomeView(context: Context): Triple<String, Boolean, Int> {
        val key = myKey(context)
        val site = Sites.all(context).firstOrNull { it.recordKey == key }
        val posts = runCatching { myFeed(context) }.getOrNull()?.posts?.size ?: 0
        return Triple(key, site != null && site.digestHex.isNotEmpty(), posts)
    }
}
