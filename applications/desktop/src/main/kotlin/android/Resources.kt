// The desk's resource system: how the phone's *screens* compile here.
//
// Shims.kt bought the protocol brain by re-creating four Android classes.
// The screens need one more thing — resources. Every phone screen reads its
// words through `stringResource(R.string.…)`, so a desk that cannot resolve
// an R id cannot host a phone screen, and a desk that duplicates the screens
// instead inherits two copies of every wording decision and all twenty
// translations. This file (with the generateDeskRes task that feeds it) is
// the cheaper half of that trade: the ids come from the phone's own XML at
// build time, and the strings come from the same files the phone ships.
//
// What the phone has and this does not: Android's resource *qualifiers* —
// densities, night mode, screen widths. Only locale matters here, so only
// locale is implemented.

package android.res

import org.json.JSONObject

object DeskRes {
    /** name → value, and for plurals name → {quantity: value}, by id. */
    private var strings: JSONObject = JSONObject()
    private var plurals: JSONObject = JSONObject()
    private var fallbackStrings: JSONObject = JSONObject()
    private var fallbackPlurals: JSONObject = JSONObject()
    private var lang: String = "en"

    /** Every language the phone ships, as the generator found them. */
    val available: List<String> by lazy {
        (load("index")?.optJSONArray("locales") ?: return@lazy listOf("en"))
            .let { a -> (0 until a.length()).map { a.getString(it) } }
    }

    private fun load(tag: String): JSONObject? =
        DeskRes::class.java.getResourceAsStream("/deskres/$tag.json")
            ?.use { JSONObject(it.readBytes().decodeToString()) }

    /**
     * Point the table at a language. A BCP-47 tag, "" for the host's own.
     * Unknown or partial languages fall through to English per *string*,
     * exactly as Android's per-resource fallback does, so a half-translated
     * language is usable rather than broken.
     */
    @Synchronized
    fun setLocale(tag: String) {
        val want = tag.ifEmpty { java.util.Locale.getDefault().toLanguageTag() }
        // "pt-BR" tries pt-BR, then pt, then English.
        val tries = listOfNotNull(
            want, want.substringBefore('-').takeIf { it != want },
        )
        val base = load("en") ?: JSONObject()
        fallbackStrings = base.optJSONObject("strings") ?: JSONObject()
        fallbackPlurals = base.optJSONObject("plurals") ?: JSONObject()
        val hit = tries.firstNotNullOfOrNull { t -> load(t)?.let { t to it } }
        lang = hit?.first ?: "en"
        strings = hit?.second?.optJSONObject("strings") ?: fallbackStrings
        plurals = hit?.second?.optJSONObject("plurals") ?: fallbackPlurals
    }

    fun string(id: Int): String {
        if (strings.length() == 0) setLocale("")
        val k = id.toString()
        return strings.optString(k, null) ?: fallbackStrings.optString(k, null) ?: "#$id"
    }

    fun string(id: Int, vararg args: Any?): String =
        format(string(id), args)

    fun plural(id: Int, count: Int): String {
        if (plurals.length() == 0) setLocale("")
        val k = id.toString()
        val set = plurals.optJSONObject(k) ?: fallbackPlurals.optJSONObject(k)
            ?: return "#$id"
        val want = quantityOf(lang, count)
        // CLDR's own fallback order: the asked-for class, then the classes a
        // language is allowed to omit, then "other", which every language has.
        for (q in listOf(want, "many", "few", "two", "one", "other")) {
            set.optString(q, null)?.let { return it }
        }
        return "#$id"
    }

    fun plural(id: Int, count: Int, vararg args: Any?): String =
        format(plural(id, count), args)

    private fun format(s: String, args: Array<out Any?>): String =
        if (args.isEmpty()) s
        else runCatching {
            String.format(java.util.Locale.getDefault(), s, *args)
        }.getOrDefault(s)

    /**
     * CLDR plural class, for the languages this app ships.
     *
     * The JDK has no plural-rules API and ICU4J is a 13 MB dependency for
     * fifteen strings, so the families are spelled out. Getting this wrong
     * is not cosmetic — "1 notes" is the kind of thing that tells someone
     * the software was not written for them.
     */
    fun quantityOf(lang: String, n: Int): String {
        val l = lang.substringBefore('-')
        return when (l) {
            // No grammatical plural at all.
            "ja", "ko", "zh", "th", "vi", "id", "fa", "tr" -> "other"
            // One vs the rest, with French/Portuguese counting 0 as one.
            "fr", "pt" -> if (n == 0 || n == 1) "one" else "other"
            "en", "de", "nl", "es", "it", "hi" -> if (n == 1) "one" else "other"
            "ru", "uk" -> {
                val m10 = n % 10
                val m100 = n % 100
                when {
                    m10 == 1 && m100 != 11 -> "one"
                    m10 in 2..4 && m100 !in 12..14 -> "few"
                    else -> "many"
                }
            }
            "pl" -> {
                val m10 = n % 10
                val m100 = n % 100
                when {
                    n == 1 -> "one"
                    m10 in 2..4 && m100 !in 12..14 -> "few"
                    else -> "many"
                }
            }
            "ar" -> {
                val m100 = n % 100
                when {
                    n == 0 -> "zero"
                    n == 1 -> "one"
                    n == 2 -> "two"
                    m100 in 3..10 -> "few"
                    m100 in 11..99 -> "many"
                    else -> "other"
                }
            }
            else -> if (n == 1) "one" else "other"
        }
    }
}
