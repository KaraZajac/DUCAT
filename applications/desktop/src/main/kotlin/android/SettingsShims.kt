// The Activity a screen reaches for when a permission has been refused for
// good — the only object that knows whether asking again would show the user
// anything at all.
//
// A desk has none: its context is a DeskContext, so the `as?` that reaches
// for an Activity comes back null and the shared screen takes its other
// path. This exists so that file compiles, not so the desk behaves like a
// phone — and it is abstract precisely because nothing here should ever
// construct one.

package android.app

abstract class Activity : android.content.Context() {
    open fun shouldShowRequestPermissionRationale(permission: String): Boolean = false
}
