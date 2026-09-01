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
    )

    private fun prefs(context: Context) = securePrefs(context, "ducat_sites")

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
            )
        }
    }

    private fun save(context: Context, sites: List<Site>) {
        val arr = JSONArray()
        for (s in sites) {
            arr.put(
                JSONObject()
                    .put("rec", s.recordKey).put("title", s.title)
                    .put("share", s.share).put("digest", s.digestHex)
                    .put("updated", s.updated).put("added", s.addedAt)
                    .put("keep", s.keepAlive)
                    .put("fetched", s.fetchedDigestHex ?: "")
                    .put("fetched_share", s.fetchedShare ?: ""),
            )
        }
        prefs(context).edit().putString("sites", arr.toString()).apply()
        ContactStore.bump()
    }

    fun bundleDir(context: Context, recordKey: String): File =
        File(context.filesDir, "sites/${recordKey.hashCode().toUInt()}/current")

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
        )
        save(context, rest + entry)
        return entry
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
        if (site.keepAlive) reseed(context, site.recordKey)
        return dir
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
        save(
            context,
            all(context).map {
                if (it.recordKey == recordKey) it.copy(keepAlive = keep) else it
            },
        )
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
        save(context, all(context).filterNot { it.recordKey == recordKey })
        File(context.filesDir, "sites/${recordKey.hashCode().toUInt()}").deleteRecursively()
    }
}
