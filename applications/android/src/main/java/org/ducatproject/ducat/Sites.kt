package org.ducatproject.ducat

import android.content.Context
import java.io.File
import org.json.JSONArray
import org.json.JSONObject

/**
 * §16.22: sites — pages that travel like publications.
 *
 * A site is one mutable head at a stable record key (`ducat:site/<key>`)
 * naming the current bundle, which rides the swarm whole and renders in a
 * sealed room. This store keeps the phone's saved sites and the publisher
 * state for sites this device owns; the bundles cache under
 * `files/sites/<id>/current`.
 */
object Sites {
    data class Site(
        val recordKey: String,
        val title: String,
        val share: String,
        val digestHex: String,
        val updated: Long,
        val addedAt: Long,
        val keepAlive: Boolean,
        val fetchedDigestHex: String?,
        // The share the cached bundle actually came from. The head's share
        // can rotate ahead of the disk; a mirror serves what it has, under
        // the key it was fetched from.
        val fetchedShare: String? = null,
        /**
         * The record's owner keypair — this site's write authority — or
         * null for the ordinary case of a site somebody else made.
         *
         * §16.22: updating a site is re-seeding a bundle and rewriting the
         * head in place, and the head is the record's subkey 0. A reader
         * opens the record with no writer at all (see [readHead]), so a
         * write is not refused by a rule anybody could relax — there is
         * nothing to sign it with. Only the holder of this can change what
         * `ducat:site/<key>` points at, which is the property a site
         * wants and also the one thing about it that cannot be replaced.
         * It rides the backup with the store (ContactStore.backupAppState).
         */
        val ownerPublic: ByteArray? = null,
        val ownerSecret: ByteArray? = null,
        /** The answers a page was written from, when it came from the
         *  form rather than a picked archive — see PageTemplate. */
        val page: String? = null,
    ) {
        /** Whether this phone can rewrite this site's head. */
        val mine: Boolean get() = ownerPublic != null && ownerSecret != null

        // A data class with ByteArray members gets identity equality for
        // them, which would make two reads of the same store unequal and
        // is a trap in a class that ends up in Compose state.
        override fun equals(other: Any?): Boolean =
            other is Site && recordKey == other.recordKey &&
                title == other.title && share == other.share &&
                digestHex == other.digestHex && updated == other.updated &&
                addedAt == other.addedAt && keepAlive == other.keepAlive &&
                fetchedDigestHex == other.fetchedDigestHex &&
                fetchedShare == other.fetchedShare &&
                ownerPublic.contentEquals(other.ownerPublic) &&
                ownerSecret.contentEquals(other.ownerSecret) && page == other.page

        override fun hashCode(): Int =
            recordKey.hashCode() * 31 + (ownerPublic?.contentHashCode() ?: 0)
    }

    private fun prefs(context: Context) = securePrefs(context, "ducat_sites")

    // Every write here is read-modify-write over the whole table, and the
    // writers run on different threads: a tapped link adding a site while
    // Open refreshes another, a fetch finishing while the checkbox flips.
    // Held for the table edit only, never across a fetch.
    private val lock = Any()

    /** `ducat:site/<record-key>` — the address for the life of the site. */
    fun uriOf(recordKey: String): String = "ducat:site/$recordKey"

    fun parseUri(uri: String): String? =
        uri.removePrefix("ducat:site/").takeIf { it != uri && it.isNotBlank() }

    fun all(context: Context): List<Site> {
        val raw = prefs(context).getString("sites", null) ?: return emptyList()
        val arr = JSONArray(raw)
        return (0 until arr.length()).map { i ->
            val o = arr.getJSONObject(i)
            Site(
                recordKey = o.getString("rec"),
                title = o.optString("title"),
                share = o.optString("share"),
                digestHex = o.optString("digest"),
                updated = o.optLong("updated"),
                addedAt = o.optLong("added"),
                keepAlive = o.optBoolean("keep", false),
                fetchedDigestHex = o.optString("fetched").ifBlank { null },
                fetchedShare = o.optString("fetched_share").ifBlank { null },
                ownerPublic = o.optString("own_pub").ifBlank { null }?.let(::unb64),
                ownerSecret = o.optString("own_sec").ifBlank { null }?.let(::unb64),
                page = o.optString("page").ifBlank { null },
            )
        }
    }

    private fun b64(b: ByteArray): String =
        android.util.Base64.encodeToString(b, android.util.Base64.NO_WRAP)

    private fun unb64(s: String): ByteArray =
        android.util.Base64.decode(s, android.util.Base64.NO_WRAP)

    // Internal rather than private for sitepublishtest, which drives the
    // store's round trip directly: publish() needs a node and the property
    // worth pinning — that a site's owner keypair survives being written
    // and read back — does not.
    internal fun save(context: Context, sites: List<Site>) {
        val arr = JSONArray()
        for (s in sites) {
            arr.put(
                JSONObject()
                    .put("rec", s.recordKey).put("title", s.title)
                    .put("share", s.share).put("digest", s.digestHex)
                    .put("updated", s.updated).put("added", s.addedAt)
                    .put("keep", s.keepAlive)
                    .put("fetched", s.fetchedDigestHex ?: "")
                    .put("fetched_share", s.fetchedShare ?: "")
                    .put("own_pub", s.ownerPublic?.let(::b64) ?: "")
                    .put("own_sec", s.ownerSecret?.let(::b64) ?: "")
                    .put("page", s.page ?: ""),
            )
        }
        prefs(context).edit().putString("sites", arr.toString()).apply()
        ContactStore.bump()
    }

    /**
     * Where a site's bundle lives, named by a digest of its address.
     *
     * This was `recordKey.hashCode()`, which is thirty-two bits of a
     * non-cryptographic hash standing between one publisher's pages and
     * another's. Two addresses that collide share a directory, and the
     * consequences are not "a cache miss": `fetchBundle` returns the cached
     * directory whenever the *site's own* fetched digest matches its head,
     * so the second site's bundle is served under the first site's address,
     * in the sealed room, with nothing on screen to say so. `remove` then
     * deletes the other one's files.
     *
     * Chance alone would rarely do it. But `String.hashCode` is trivial to
     * aim: somebody who wants their pages rendered as yours grinds record
     * keys until the low thirty-two bits agree, and substitution is exactly
     * the attack §16.22's room is built against ([Posters] says the same
     * about boards). Every other cache here is named by a full identifier —
     * `publications/<publisherHex>/<period>`, `swarm_out/<digestHex>` — and
     * this was the one place that was not.
     */
    fun bundleDir(context: Context, recordKey: String): File =
        File(context.filesDir, "sites/${dirNameOf(recordKey)}/current")

    private fun dirNameOf(recordKey: String): String =
        java.security.MessageDigest.getInstance("SHA-256")
            .digest(recordKey.toByteArray()).toHexString()

    /**
     * Drop bundle directories no saved site claims.
     *
     * Two of them: the ones the old name left behind, which no longer
     * answer to anything and would otherwise sit on the phone for good, and
     * any bundle whose site went away while its files did not. A site whose
     * cache is missing re-fetches on the next open, so removing too much
     * costs a download and never a wrong page.
     */
    fun sweepOrphans(context: Context): Int {
        val root = File(context.filesDir, "sites")
        if (!root.isDirectory) return 0
        val keep = all(context).mapTo(HashSet()) { dirNameOf(it.recordKey) }
        val gone = root.listFiles()?.filter { it.isDirectory && it.name !in keep } ?: return 0
        for (d in gone) d.deleteRecursively()
        if (gone.isNotEmpty()) {
            DucatLog.i("Sites", "swept ${gone.size} orphaned bundle(s)")
        }
        return gone.size
    }

    /** Read the head at a record key — the resolve a saved or pasted
     *  address runs. The record must be openable read-only. */
    fun readHead(recordKey: String): uniffi.ducat_mobile.SiteHeadIo {
        uniffi.ducat_mobile.nodeDhtOpen(recordKey, null, null)
        val bytes = uniffi.ducat_mobile.nodeDhtGet(recordKey, 0u, true)
            ?: throw IllegalStateException("the site's head answered nothing")
        return uniffi.ducat_mobile.siteHeadDecode(bytes)
    }

    /** Add (or refresh) a site by address; returns the stored entry. */
    fun add(context: Context, recordKey: String): Site {
        val head = readHead(recordKey)
        val now = System.currentTimeMillis() / 1000
        return synchronized(lock) {
            val rest = all(context).filterNot { it.recordKey == recordKey }
            val prior = all(context).firstOrNull { it.recordKey == recordKey }
            val entry = Site(
                recordKey = recordKey,
                title = head.title,
                share = head.share,
                digestHex = head.digestHex,
                updated = head.updated.toLong(),
                addedAt = prior?.addedAt ?: now,
                keepAlive = prior?.keepAlive ?: false,
                fetchedDigestHex = prior?.fetchedDigestHex,
                fetchedShare = prior?.fetchedShare,
                // Carried, and the reason is the whole of §16.22's ownership
                // model: this rebuilds the row from the head, and the head is
                // public. A publisher who pastes their own address — or taps
                // a ducat:site link to their own page, which is the obvious
                // way to check it looks right — would otherwise come back
                // from that with the write authority gone and no warning,
                // the site frozen for good at whatever it last said.
                ownerPublic = prior?.ownerPublic,
                ownerSecret = prior?.ownerSecret,
                page = prior?.page,
            )
            save(context, rest + entry)
            entry
        }
    }

    /** Fetch the current bundle if the cache is stale; returns the dir.
     *  stay follows the site's own keep-alive choice (§16.22: mirroring
     *  is a gift knowingly given). */
    fun fetchBundle(context: Context, site: Site): File {
        val dir = bundleDir(context, site.recordKey)
        if (site.fetchedDigestHex == site.digestHex && dir.isDirectory &&
            dir.walkTopDown().any { it.isFile }
        ) {
            return dir
        }
        val fresh = File(dir.parentFile, "next")
        fresh.deleteRecursively(); fresh.mkdirs()
        // Never staySeeding here: a seed parked now would be rooted at
        // `next/`, and the rename below pulls the floor out from under it.
        // The park happens after the swap, from the dir that will last.
        Swarm.fetch(site.share, site.digestHex, fresh.absolutePath)
        Swarm.stopShare(site.fetchedShare ?: site.share)
        dir.deleteRecursively()
        check(fresh.renameTo(dir)) { "could not move the site into place" }
        synchronized(lock) {
            save(
                context,
                all(context).map {
                    if (it.recordKey == site.recordKey) {
                        it.copy(
                            fetchedDigestHex = site.digestHex,
                            fetchedShare = site.share,
                        )
                    } else {
                        it
                    }
                },
            )
        }
        // The choice as it stands now, not as it stood when the fetch began.
        reseed(context, site.recordKey)
        return dir
    }

    // ----- the publisher's half (§16.22) -----------------------------------

    /**
     * Which of this phone's own pages the Pages mode is showing.
     *
     * Press's `pressPub` exactly, and self-healing the same way: a stored
     * id that no longer names a page this phone owns reads as unset, so
     * deleting the page that was forward does not leave the mode pointing
     * at nothing.
     */
    fun frontPage(context: Context): String? =
        prefs(context).getString("front", null)
            ?.takeIf { key -> all(context).any { it.recordKey == key && it.mine } }

    fun setFrontPage(context: Context, recordKey: String) {
        prefs(context).edit().putString("front", recordKey).apply()
        ContactStore.bump()
    }


    /**
     * A page that reaches for the clearnet, and where.
     *
     * §16.22 says a publisher tool SHOULD refuse to seed a bundle that
     * references external resources, and gives the reasons: one external
     * fetch hands the reader's address and timing to a third party, a
     * per-visitor URL makes that targeted, an unfetched resource is
     * unsigned content inside a digest-verified page, and a bundle with
     * clearnet dependencies neither works offline nor survives its
     * publisher. The viewer already refuses these at render (every
     * request is answered from the bundle or not at all), so this is not
     * a second wall — it is the wall being hit at the keyboard, by the
     * one person who can fix it, instead of silently on a stranger's
     * phone where the page just looks broken.
     *
     * Returns null when the bundle is sealed, or "<file>: <the offending
     * text>" for the first thing that is not.
     */
    fun clearnetIn(dir: File): String? {
        val external = Regex(
            """(?i)(src|href)\s*=\s*["'](https?:)?//|""" +
                """url\(\s*["']?(https?:)?//|@import\s+["'](https?:)?//""",
        )
        for (f in dir.walkTopDown()) {
            if (!f.isFile) continue
            if (f.extension.lowercase() !in setOf("html", "htm", "css", "svg")) continue
            val hit = external.find(runCatching { f.readText() }.getOrDefault("")) ?: continue
            return "${f.relativeTo(dir)}: ${hit.value.trim()}"
        }
        return null
    }

    /**
     * Publish [dir] as this phone's site, or update one it already owns.
     *
     * The desk has done this since §16.22 landed (desk/SitePublish.kt) and
     * this is the same four steps: lint, seed the bundle, mint or reopen
     * the record, write the head. What differs is only that a phone keeps
     * the owner keypair in its own store rather than a file beside the
     * bundle, and that [recordKey] being non-null is what makes this an
     * update — same record, same address, new bundle, head rewritten in
     * place. Readers who saved the address are not disturbed by that;
     * §16.22 is explicit that they keep it across every update.
     *
     * The bundle stays seeded from here: unlike an issue, which is
     * delivered into a thread and then owned by whoever holds it, a site
     * exists only while somebody serves it. This phone is the origin
     * until a mirror takes over or the site goes quiet.
     */
    fun publish(
        context: Context,
        source: File,
        title: String,
        recordKey: String? = null,
        /** The form answers this page was written from, when it was.
         *  Kept so the composer can offer them back — a page written
         *  from a picked archive has none and passes null. */
        page: String? = null,
    ): Site {
        require(File(source, "index.html").isFile) { "a site needs an index.html at its root" }
        clearnetIn(source)?.let {
            throw IllegalArgumentException("that page reaches the network — $it")
        }

        // 1. The address, first, because everything below is named after it.
        val prior = recordKey?.let { k -> all(context).firstOrNull { it.recordKey == k } }
        val key: String
        val pub: ByteArray
        val sec: ByteArray
        if (prior != null && prior.mine) {
            key = prior.recordKey
            pub = prior.ownerPublic!!
            sec = prior.ownerSecret!!
            uniffi.ducat_mobile.nodeDhtOpen(key, pub, sec)
        } else {
            // One subkey: the head is subkey 0 and the record holds nothing
            // else. Minted once per site and never again — the record key
            // *is* the address, so a second mint is a second site.
            val rec = uniffi.ducat_mobile.nodeDhtCreate(1u)
            key = rec.key
            pub = rec.ownerPublic
            sec = rec.ownerSecret
        }

        // 2. Committed before anything else can fail, and the order is the
        //    one Orders.bind argues for. A death after this costs a record
        //    with no head yet, which the next publish rewrites. A death
        //    before it, having minted, would cost a record this phone owns
        //    and can no longer prove it owns: an address that answers
        //    nothing, for ever, with the only key that could fix it gone.
        val now = System.currentTimeMillis() / 1000
        val base = Site(
            recordKey = key,
            title = title,
            share = prior?.share.orEmpty(),
            digestHex = prior?.digestHex.orEmpty(),
            updated = now,
            addedAt = prior?.addedAt ?: now,
            // A publisher mirrors their own site by definition; the
            // checkbox is about hosting somebody else's.
            keepAlive = true,
            fetchedDigestHex = prior?.fetchedDigestHex,
            fetchedShare = prior?.fetchedShare,
            ownerPublic = pub,
            ownerSecret = sec,
            page = page ?: prior?.page,
        )
        synchronized(lock) {
            save(context, all(context).filterNot { it.recordKey == key } + base)
        }

        // 3. Into the same place a fetched bundle lives, so the viewer, the
        //    reseed and fetchBundle all treat a page we wrote exactly like
        //    one we were given. Seeded from its final path, never from a
        //    staging dir — a share parked over a directory about to be
        //    replaced is the trap fetchBundle documents.
        val dir = bundleDir(context, key)
        val fresh = File(dir.parentFile, "next")
        fresh.deleteRecursively()
        fresh.mkdirs()
        source.copyRecursively(fresh, overwrite = true)
        val old = all(context).firstOrNull { it.recordKey == key }?.fetchedShare
        if (old != null) runCatching { Swarm.stopShare(old) }
        dir.deleteRecursively()
        check(fresh.renameTo(dir)) { "could not move the page into place" }
        val share = Swarm.seed(dir.absolutePath)

        // 4. The head last: until it is written the address points at
        //    nothing, and after it the whole network can read the new page.
        val entry = base.copy(
            share = share.shareKey,
            digestHex = share.indexDigestHex,
            fetchedDigestHex = share.indexDigestHex,
            fetchedShare = share.shareKey,
        )
        synchronized(lock) {
            save(context, all(context).filterNot { it.recordKey == key } + entry)
        }
        uniffi.ducat_mobile.nodeDhtSet(
            key,
            0u,
            uniffi.ducat_mobile.siteHeadEncode(
                uniffi.ducat_mobile.SiteHeadIo(
                    title = title,
                    share = share.shareKey,
                    digestHex = share.indexDigestHex,
                    updated = now.toULong(),
                ),
            ),
        )
        DucatLog.i(
            "Sites",
            "published '$title' at ${uriOf(key)} " +
                "(${if (prior?.mine == true) "update" else "new"})",
        )
        return entry
    }

    /**
     * Put a kept site's cached bundle back into serving — a verify-only
     * stay fetch over complete files that downloads nothing (§16.20's
     * restart-reseed primitive, same as the shelf and the outbox). This
     * is what makes the keep-alive checkbox a promise rather than a
     * mood: it runs after every fetch, when the box is ticked, and once
     * per process start from the poller.
     */
    fun reseed(context: Context, recordKey: String) {
        val site = all(context).firstOrNull { it.recordKey == recordKey } ?: return
        if (!site.keepAlive) return
        val digest = site.fetchedDigestHex ?: return
        val share = site.fetchedShare ?: site.share
        val dir = bundleDir(context, recordKey)
        if (!dir.isDirectory || !dir.walkTopDown().any { it.isFile }) return
        Thread {
            runCatching {
                // Stop-then-stay: parking the same share twice would strand
                // the first task; stopping a share nobody serves is a no-op.
                Swarm.stopShare(share)
                Swarm.fetch(share, digest, dir.absolutePath, staySeeding = true)
            }
        }.apply { isDaemon = true; name = "site-reseed" }.start()
    }

    fun setKeepAlive(context: Context, recordKey: String, keep: Boolean) {
        synchronized(lock) {
            save(
                context,
                all(context).map {
                    if (it.recordKey == recordKey) it.copy(keepAlive = keep) else it
                },
            )
        }
        // The checkbox acts now, not at the next fetch: ticking parks the
        // bundle already on disk, unticking stops serving it.
        if (keep) {
            reseed(context, recordKey)
        } else {
            all(context).firstOrNull { it.recordKey == recordKey }?.let {
                Swarm.stopShare(it.fetchedShare ?: it.share)
            }
        }
    }

    fun remove(context: Context, recordKey: String) {
        all(context).firstOrNull { it.recordKey == recordKey }?.let {
            // A parked seeder over deleted files would linger until the
            // process dies; stop it before the bundle goes.
            Swarm.stopShare(it.fetchedShare ?: it.share)
        }
        synchronized(lock) {
            save(context, all(context).filterNot { it.recordKey == recordKey })
        }
        File(context.filesDir, "sites/${dirNameOf(recordKey)}").deleteRecursively()
    }
}
