package org.ducatproject.desk

import java.io.File
import java.util.Base64
import org.json.JSONObject
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus

/**
 * Publish a directory as a ducat site (§16.22): lint the sealed-room rule,
 * seed the bundle, write the head, print the address, stay serving.
 *
 *   DUCAT_SITE_DIR=<dir> DUCAT_SITE_STATE=<state> DUCAT_SITE_TITLE=<t> \
 *     ./gradlew :desktop:sitepublish
 *
 * Re-running with the same state updates the site in place: same record
 * key, new bundle, head rewritten. The state dir holds the record's owner
 * keypair — the site's write authority — so treat it like one.
 */

private val EXTERNAL = Regex(
    """(?i)(src|href)\s*=\s*["'](https?:)?//|url\(\s*["']?(https?:)?//|@import\s+["'](https?:)?//""",
)

fun main() {
    val siteDir = File(System.getenv("DUCAT_SITE_DIR") ?: error("set DUCAT_SITE_DIR"))
    check(siteDir.isDirectory) { "SITE_FAIL not a directory: $siteDir" }
    check(File(siteDir, "index.html").isFile) { "SITE_FAIL no index.html at the root" }
    val stateDir = File(System.getenv("DUCAT_SITE_STATE") ?: error("set DUCAT_SITE_STATE"))
        .apply { mkdirs() }
    val title = System.getenv("DUCAT_SITE_TITLE") ?: "Untitled site"

    // The sealed-room lint (§16.22): a page that references the clearnet
    // is broken by design — say so at the keyboard, not on a stranger's
    // phone.
    siteDir.walkTopDown().filter {
        it.isFile && it.extension.lowercase() in setOf("html", "htm", "css", "svg")
    }.forEach { f ->
        val hit = EXTERNAL.find(f.readText())
        check(hit == null) {
            "SITE_FAIL ${f.relativeTo(siteDir)} references the network " +
                "('${hit!!.value.trim()}…') — a ducat site is a sealed room; " +
                "bundle every asset."
        }
    }
    println("SITE_LINT_OK")

    val context = DeskContext(stateDir)
    nodeStart("${stateDir.absolutePath}/veilid", true)
    val deadline = System.currentTimeMillis() + 240_000
    while (System.currentTimeMillis() < deadline && !nodeStatus().publicInternetReady) {
        Thread.sleep(2_000)
    }
    check(nodeStatus().publicInternetReady) { "SITE_FAIL node never became ready" }
    System.err.println("node ready")

    val share = org.ducatproject.ducat.Swarm.seed(siteDir.absolutePath)
    println("SITE_BUNDLE ${share.shareKey} ${share.indexDigestHex}")

    // The record: created once, reopened with its owner keys for ever after.
    val recFile = File(stateDir, "site-record.json")
    val recordKey: String
    if (recFile.isFile) {
        val o = JSONObject(recFile.readText())
        recordKey = o.getString("key")
        uniffi.ducat_mobile.nodeDhtOpen(
            recordKey,
            Base64.getDecoder().decode(o.getString("pub")),
            Base64.getDecoder().decode(o.getString("sec")),
        )
    } else {
        val rec = uniffi.ducat_mobile.nodeDhtCreate(1u)
        recordKey = rec.key
        recFile.writeText(
            JSONObject()
                .put("key", rec.key)
                .put("pub", Base64.getEncoder().encodeToString(rec.ownerPublic))
                .put("sec", Base64.getEncoder().encodeToString(rec.ownerSecret))
                .toString(),
        )
    }
    val head = uniffi.ducat_mobile.siteHeadEncode(
        uniffi.ducat_mobile.SiteHeadIo(
            title = title,
            share = share.shareKey,
            digestHex = share.indexDigestHex,
            updated = (System.currentTimeMillis() / 1000).toULong(),
        ),
    )
    uniffi.ducat_mobile.nodeDhtSet(recordKey, 0u, head)
    println("SITE_URI ducat:site/$recordKey")
    System.out.flush()
    // Stay up as the origin until stopped; mirrors take over from here.
    val serveMin = System.getenv("DUCAT_SITE_SERVE_MIN")?.toLongOrNull() ?: 30
    Thread.sleep(serveMin * 60_000)
}
