package org.ducatproject.ducat

import android.annotation.SuppressLint
import android.os.Bundle
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.activity.ComponentActivity
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
        val rootCanon = root.canonicalPath

        val web = WebView(this)
        web.settings.apply {
            javaScriptEnabled = false
            blockNetworkLoads = true
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
                        startActivity(
                            android.content.Intent(
                                android.content.Intent.ACTION_VIEW, url,
                            ).setPackage(packageName),
                        )
                        true
                    }
                    url.host == "site.local" -> false
                    else -> true // swallowed; external links do not exist here
                }
            }
        }
        setContentView(web)
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
        "woff", "woff2" -> "font/woff2"
        "txt", "md" -> "text/plain"
        "json" -> "application/json"
        else -> "application/octet-stream"
    }
}
