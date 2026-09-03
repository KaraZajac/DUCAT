package org.ducatproject.ducat

import android.annotation.SuppressLint
import android.os.Bundle
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.activity.ComponentActivity
import androidx.activity.addCallback
import java.io.ByteArrayInputStream
import java.io.File

/**
 * §16.22's sealed room. Every resource request is answered from the
 * fetched bundle and nothing else — no network of any kind from rendered
 * content, scripts off, `ducat:` links the only door out. This is the
 * renderer's architecture, not a policy a page could argue with: one
 * external fetch would hand the reader's address to a third party, and a
 * per-visitor beacon would make it targeted.
 */
class SiteViewerActivity : ComponentActivity() {
    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val recordKey = intent.getStringExtra("record") ?: run { finish(); return }
        val root = Sites.bundleDir(this, recordKey)
        if (!File(root, "index.html").isFile) {
            finish(); return
        }
        // With the separator: a bare prefix let `../current-anything/` next
        // to the bundle pass as inside it.
        val rootCanon = root.canonicalPath + File.separator

        val web = WebView(this)
        web.settings.apply {
            javaScriptEnabled = false
            blockNetworkLoads = true
            // Safe Browsing checks every navigation against Google before
            // it loads — one external fetch from a room whose whole point
            // is having none.
            if (android.os.Build.VERSION.SDK_INT >= 26) safeBrowsingEnabled = false
            // Not blockNetworkImage: that one short-circuits on the URL
            // scheme before shouldInterceptRequest is consulted, so it
            // starves the bundle's own images. blockNetworkLoads and the
            // intercept-everything client are the walls; an image request
            // still cannot reach the network.
            allowFileAccess = false
            allowContentAccess = false
            domStorageEnabled = false
            setGeolocationEnabled(false)
        }
        web.webViewClient = object : WebViewClient() {
            override fun shouldInterceptRequest(
                view: WebView,
                request: WebResourceRequest,
            ): WebResourceResponse {
                val url = request.url
                if (url.host == "site.local") {
                    val rel = url.path?.trimStart('/')?.ifBlank { "index.html" }
                        ?: "index.html"
                    val f = File(root, rel)
                    // The one wall that matters twice: inside the bundle,
                    // and only the bundle.
                    if (f.canonicalPath.startsWith(rootCanon) && f.isFile) {
                        return WebResourceResponse(mimeFor(rel), null, f.inputStream())
                    }
                }
                // Everything else — any scheme, any host — is a closed door.
                return WebResourceResponse(
                    "text/plain", "utf-8", ByteArrayInputStream(ByteArray(0)),
                )
            }

            override fun shouldOverrideUrlLoading(
                view: WebView,
                request: WebResourceRequest,
            ): Boolean {
                val url = request.url
                return when {
                    url.scheme == "ducat" -> {
                        // The page's only working exits: hand the URI to
                        // the app's own flows and stay open behind them.
                        // Only a tap on the page itself: a meta refresh
                        // or a frame can name a ducat: address too, and
                        // that is a page opening doors without a hand on
                        // them.
                        if (request.hasGesture() && request.isForMainFrame) {
                            startActivity(
                                android.content.Intent(
                                    android.content.Intent.ACTION_VIEW, url,
                                ).setPackage(packageName)
                                    // …and stay open behind them, which is
                                    // what the comment above promises and
                                    // what did not happen: MainActivity is
                                    // singleTop in the same task, so this
                                    // brought it to the front and finished
                                    // the room. Following a link out of a
                                    // page ended the page, and Back went to
                                    // wherever the app had been rather than
                                    // to what was being read. A new task
                                    // keeps the room where it was.
                                    .addFlags(
                                        android.content.Intent.FLAG_ACTIVITY_NEW_TASK or
                                            android.content.Intent.FLAG_ACTIVITY_NEW_DOCUMENT,
                                    ),
                            )
                        }
                        true
                    }
                    url.host == "site.local" -> false
                    else -> true // swallowed; external links do not exist here
                }
            }
        }
        setContentView(web)
        // A site with more than one page has a way back through it; the
        // system gesture leaves the room only from its first page.
        onBackPressedDispatcher.addCallback(this) {
            if (web.canGoBack()) web.goBack() else finish()
        }
        web.loadUrl("https://site.local/index.html")
    }

    private fun mimeFor(path: String): String = when (path.substringAfterLast('.').lowercase()) {
        "html", "htm" -> "text/html"
        "css" -> "text/css"
        "png" -> "image/png"
        "jpg", "jpeg" -> "image/jpeg"
        "gif" -> "image/gif"
        "svg" -> "image/svg+xml"
        "webp" -> "image/webp"
        "woff" -> "font/woff"
        "woff2" -> "font/woff2"
        "txt", "md" -> "text/plain"
        "json" -> "application/json"
        else -> "application/octet-stream"
    }
}
