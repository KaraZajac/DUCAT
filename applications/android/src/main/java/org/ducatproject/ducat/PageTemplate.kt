package org.ducatproject.ducat

import java.io.File

/**
 * A page, written for somebody who does not write pages.
 *
 * §16.22's bundle is a directory of HTML, which on a laptop is a
 * reasonable thing to ask for and on a phone is a wall. The zip route
 * exists for people who already have a bundle; this is for the shop that
 * wants an address to put in a window, and it is the reason Pages is a
 * mode rather than a developer tool.
 *
 * Generating the markup also settles the sealed-room rule by
 * construction rather than by inspection. §16.22 requires a viewer to
 * answer every request from the bundle and nothing else, and asks a
 * publisher tool to refuse a bundle that reaches the clearnet; a page
 * assembled here has no external reference to refuse, because nothing
 * ever puts one in. [Sites.clearnetIn] still runs over the result — a
 * lint you only apply to other people's work is a lint you find out is
 * broken on other people's work.
 *
 * Everything is escaped on the way in. The fields are typed by the
 * publisher, so this is not a security boundary in the way a stranger's
 * bytes would be, but a shop called "Bea & Sons" should not have to
 * discover HTML entities to get an apostrophe onto its own page.
 */
object PageTemplate {

    /** What a person fills in. Every field but [title] may be blank. */
    data class Page(
        val title: String,
        val tagline: String = "",
        val body: String = "",
        val hours: String = "",
        val contact: String = "",
        /** `ducat:` addresses only — the page's one working exit (§16.22),
         *  and the reason a shop's page can hand over a card or a
         *  publication without the reader leaving the sealed room. */
        val links: List<Pair<String, String>> = emptyList(),
    )

    /**
     * What was typed, kept as what was typed.
     *
     * The HTML is an artifact; these fields are the source. Storing only
     * the artifact meant re-entering the room offered a blank form over a
     * live page, and Update would have published the blank — the fields
     * cleared the moment the site existed and nothing could put them back,
     * because a generated page cannot be read back into the boxes it came
     * from without parsing our own markup, which is a worse idea than
     * simply keeping the answers.
     */
    fun toJson(p: Page): String =
        org.json.JSONObject()
            .put("title", p.title).put("tagline", p.tagline)
            .put("body", p.body).put("hours", p.hours).put("contact", p.contact)
            .toString()

    fun fromJson(s: String?): Page? =
        s?.takeIf { it.isNotBlank() }?.let {
            runCatching {
                val o = org.json.JSONObject(it)
                Page(
                    title = o.optString("title"),
                    tagline = o.optString("tagline"),
                    body = o.optString("body"),
                    hours = o.optString("hours"),
                    contact = o.optString("contact"),
                )
            }.getOrNull()
        }

    private fun esc(s: String): String =
        s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
            .replace("\"", "&quot;").replace("'", "&#39;")

    /**
     * Paragraphs from blank-line-separated text, so somebody typing into
     * a phone keyboard gets the shape they typed rather than one run-on
     * block. Single newlines become breaks; the escaping happens first,
     * so no markup a person types can survive as markup.
     */
    private fun paragraphs(s: String): String =
        s.trim().split(Regex("\n\\s*\n")).filter { it.isNotBlank() }.joinToString("\n") { p ->
            "    <p>${esc(p.trim()).replace("\n", "<br>")}</p>"
        }

    /**
     * Only `ducat:` gets through, and this is the load-bearing line.
     *
     * A link is the one thing on this page that points anywhere, and
     * §16.22 is explicit that `ducat:` URIs are a page's only working
     * exits — the viewer swallows everything else. Left open, a field
     * labelled "link" would collect `https://` addresses that render as
     * links, look like links, and do nothing when tapped, which is worse
     * than refusing them. `javascript:` is in the same bucket and worth
     * naming: scripts are off in the viewer, so it too would be a dead
     * thing that looked alive.
     */
    fun usableLink(href: String): Boolean =
        href.startsWith("ducat:") && !href.contains('\n') && !href.contains('"')

    /**
     * The bundle: `index.html` and nothing else, because everything the
     * page needs is in it. No stylesheet file — the CSS is inline for the
     * same reason there are no images yet, that every extra file is
     * another thing to get into the swarm share and another thing that
     * can arrive half-fetched.
     */
    fun write(page: Page, into: File) {
        into.mkdirs()
        File(into, "index.html").writeText(html(page))
    }

    fun html(p: Page): String {
        val links = p.links.filter { usableLink(it.second) }
        return buildString {
            append("<!doctype html>\n<html lang=\"en\">\n<head>\n")
            append("<meta charset=\"utf-8\">\n")
            append("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n")
            append("<title>${esc(p.title)}</title>\n")
            // A single system stack: a webfont would be a file to bundle
            // at best and a request to the clearnet at worst.
            append(
                "<style>\n" +
                    ":root{color-scheme:light dark}\n" +
                    "body{font:16px/1.55 -apple-system,system-ui,\"Segoe UI\",Roboto," +
                    "sans-serif;margin:0;padding:2rem 1.25rem;max-width:34rem;" +
                    "margin-inline:auto;background:#fbfaf8;color:#1c1b19}\n" +
                    "h1{font-size:1.7rem;line-height:1.2;margin:0 0 .25rem}\n" +
                    ".tagline{color:#6b6660;margin:0 0 1.5rem}\n" +
                    "section{margin:1.5rem 0}\n" +
                    "h2{font-size:.8rem;text-transform:uppercase;letter-spacing:.08em;" +
                    "color:#6b6660;margin:0 0 .4rem}\n" +
                    "a{color:#7a5c2e}\n" +
                    "ul{list-style:none;padding:0;margin:0}\n" +
                    "li{margin:.35rem 0}\n" +
                    "@media(prefers-color-scheme:dark){body{background:#171614;color:#e8e4de}" +
                    ".tagline,h2{color:#9b958c}a{color:#d3ae74}}\n" +
                    "</style>\n",
            )
            append("</head>\n<body>\n")
            append("<h1>${esc(p.title)}</h1>\n")
            if (p.tagline.isNotBlank()) append("<p class=\"tagline\">${esc(p.tagline)}</p>\n")
            if (p.body.isNotBlank()) append("<section>\n${paragraphs(p.body)}\n</section>\n")
            if (p.hours.isNotBlank()) {
                append("<section>\n  <h2>Hours</h2>\n${paragraphs(p.hours)}\n</section>\n")
            }
            if (p.contact.isNotBlank()) {
                append("<section>\n  <h2>Find us</h2>\n${paragraphs(p.contact)}\n</section>\n")
            }
            if (links.isNotEmpty()) {
                append("<section>\n  <h2>In DUCAT</h2>\n  <ul>\n")
                for ((label, href) in links) {
                    append("    <li><a href=\"${esc(href)}\">${esc(label)}</a></li>\n")
                }
                append("  </ul>\n</section>\n")
            }
            append("</body>\n</html>\n")
        }
    }
}
