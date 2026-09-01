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
                    .put("fetched", s.fetchedDigestHex ?: ""),
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
        Swarm.fetch(site.share, site.digestHex, fresh.absolutePath, staySeeding = site.keepAlive)
        dir.deleteRecursively()
        check(fresh.renameTo(dir)) { "could not move the site into place" }
        save(
            context,
            all(context).map {
                if (it.recordKey == site.recordKey) {
                    it.copy(fetchedDigestHex = site.digestHex)
                } else {
                    it
                }
            },
        )
        return dir
    }

    fun setKeepAlive(context: Context, recordKey: String, keep: Boolean) {
        save(
            context,
            all(context).map {
                if (it.recordKey == recordKey) it.copy(keepAlive = keep) else it
            },
        )
    }

    fun remove(context: Context, recordKey: String) {
        save(context, all(context).filterNot { it.recordKey == recordKey })
        File(context.filesDir, "sites/${recordKey.hashCode().toUInt()}").deleteRecursively()
    }
}
