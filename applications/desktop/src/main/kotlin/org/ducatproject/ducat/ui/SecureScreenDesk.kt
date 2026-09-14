package org.ducatproject.ducat.ui

/**
 * The phone's [SecureWhileShown], desk edition: nothing.
 *
 * On the phone it sets FLAG_SECURE on the window a PIN or a passphrase is
 * typed in, so the secret stays out of screenshots and recordings. A desk
 * window has no such flag to set, and the shared screens carry the call
 * unchanged, so this answers it with silence rather than a stub that
 * pretends.
 */
@androidx.compose.runtime.Composable
fun SecureWhileShown() {
}
