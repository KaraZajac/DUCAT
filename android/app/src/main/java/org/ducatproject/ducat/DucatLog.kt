package org.ducatproject.ducat

import android.util.Log
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * What the app has been doing, where a person holding the phone can read it.
 *
 * Everything so far went to logcat, which needs a cable and a desktop. Every
 * time this app has misbehaved the first question was "why", and the answer was
 * sitting somewhere the person seeing the problem could not reach — so the
 * diagnosis went through guesses instead.
 *
 * A ring buffer rather than logcat: this keeps only our lines, keeps them
 * across a screen change, and can be copied out in one tap. Logcat also drops
 * our entries under load, which is exactly when they matter.
 *
 * **Nothing secret goes in here.** Keys, seeds, message bodies and passphrases
 * stay out, because the whole point of this buffer is that it gets pasted into
 * a chat window — see [redact].
 */
object DucatLog {
    private const val CAP = 400

    enum class Level { Info, Warn, Error }

    data class Entry(val at: Long, val level: Level, val tag: String, val message: String)

    private val lines = ArrayDeque<Entry>(CAP)
    private val _changes = MutableStateFlow(0L)

    /** Bumped on every entry, so a screen showing them can follow along. */
    val changes: StateFlow<Long> = _changes

    fun i(tag: String, msg: String) = add(Level.Info, tag, msg)
    fun w(tag: String, msg: String) = add(Level.Warn, tag, msg)
    fun e(tag: String, msg: String) = add(Level.Error, tag, msg)

    @Synchronized
    private fun add(level: Level, tag: String, msg: String) {
        val clean = redact(msg)
        // Still to logcat, so a developer with a cable is not worse off.
        when (level) {
            Level.Info -> Log.i(tag, clean)
            Level.Warn -> Log.w(tag, clean)
            Level.Error -> Log.e(tag, clean)
        }
        if (lines.size >= CAP) lines.removeFirst()
        lines.addLast(Entry(System.currentTimeMillis(), level, tag, clean))
        _changes.value = _changes.value + 1
    }

    @Synchronized
    fun snapshot(): List<Entry> = lines.toList()

    @Synchronized
    fun clear() {
        lines.clear()
        _changes.value = _changes.value + 1
    }

    /** The whole buffer as text, for pasting somewhere else. */
    fun asText(): String {
        val f = SimpleDateFormat("HH:mm:ss", Locale.US)
        return snapshot().joinToString("\n") {
            "${f.format(Date(it.at))} ${it.level.name.first()} ${it.tag}: ${it.message}"
        }
    }

    /**
     * Remove anything that should not travel.
     *
     * A log written to be pasted is a log that will be pasted, and a long hex
     * run in this app is a key far more often than it is anything else. Record
     * keys are kept short rather than dropped, because "which record" is
     * usually the question being asked.
     */
    private fun redact(s: String): String = s
        // 64+ hex characters: spend keys, view keys, key images, persona keys.
        .replace(Regex("\\b[0-9a-fA-F]{64,}\\b")) { "${it.value.take(8)}…[${it.value.length} hex]" }
        // Record keys are three base64 parts; the middle identifies it well enough.
        .replace(Regex("VLD0:([A-Za-z0-9_-]{8})[A-Za-z0-9_-]+:[A-Za-z0-9_-]+")) { "VLD0:${it.groupValues[1]}…" }
        // A whole card would carry its writer secret.
        .replace(Regex("ducat:card/\\S+"), "ducat:card/[card]")
}
