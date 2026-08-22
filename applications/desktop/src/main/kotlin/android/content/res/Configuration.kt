// A desk runs in one language for the length of its run.
//
// The phone can ask for a string in a locale that is not the current one:
// `Languages.everyTranslationOf` walks all nineteen so it can recognise a
// message body somebody else's phone composed in theirs. A desk has only the
// strings it was built with, so the honest answer to "that string, in that
// locale" is the one it has — `setLocale` records nothing and
// `createConfigurationContext` hands back the same context, which makes the
// walk return a one-element set here and nineteen on a phone.
package android.content.res

class Configuration() {
    constructor(other: Configuration) : this()
    fun setLocale(l: java.util.Locale) = Unit
}
