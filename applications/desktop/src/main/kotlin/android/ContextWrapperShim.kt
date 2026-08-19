// A context that stands in front of another one.
//
// Android hands screens a chain of these and leaves it to the caller to walk
// back to the activity underneath. A desk has no chain — its context is the
// one object there is — so nothing here ever wraps anything, and the walk
// that shared code does ends immediately. Declared so that walk compiles.

package android.content

abstract class ContextWrapper : Context() {
    open val baseContext: Context? get() = null
}
