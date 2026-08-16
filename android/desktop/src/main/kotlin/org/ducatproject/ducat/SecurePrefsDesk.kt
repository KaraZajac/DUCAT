package org.ducatproject.ducat

import android.content.Context
import android.content.SharedPreferences

/**
 * The desk's `securePrefs`, matching the phone's signature so the shared
 * stores compile against both.
 *
 * Plaintext for now, deliberately: the desktop has no Android Keystore, and a
 * laptop's at-rest story is its own disk encryption, not this. Encrypting the
 * desk's files is a separate follow-up with a different key source; until then
 * this is an honest passthrough rather than a fake sense of protection.
 */
fun securePrefs(context: Context, name: String): SharedPreferences =
    context.getSharedPreferences(name, 0)
