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
    private const val CAP = 600
    /** The on-disk tail: enough for a day of testing, small enough to share. */
    private const val FILE_CAP_BYTES = 512 * 1024

    @Volatile
    private var file: java.io.File? = null

    /**
     * Start persisting, install the crash hook, stamp the build.
     *
     * **Crash capture is the point.** The log that matters most in a field
     * test is the one written in the last second before the process dies, and
     * an in-memory ring loses exactly that one. The handler writes the stack
     * through to disk (the append is synchronous) before handing the crash to
     * the system, so the next launch opens with the evidence on screen.
     */
    fun init(context: android.content.Context) {
        val f = java.io.File(context.filesDir, "ducat.log")
        file = f
        // Reload the tail, so a restart — crash or otherwise — keeps history.
        runCatching {
            if (f.length() > FILE_CAP_BYTES) {
                val tail = f.readBytes().let { it.copyOfRange(it.size - FILE_CAP_BYTES / 2, it.size) }
                f.writeBytes(tail)
            }
            f.readLines().takeLast(CAP).forEach { line ->
                parseLine(line)?.let {
                    if (lines.size >= CAP) lines.removeFirst()
                    lines.addLast(it)
                }
            }
        }
        val prior = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { t, e ->
            runCatching {
                add(
                    Level.Error, "Crash",
                    "${e.javaClass.simpleName}: ${e.message} on ${t.name}\n" +
                        e.stackTrace.take(20).joinToString("\n") { "  at $it" },
                )
            }
            prior?.uncaughtException(t, e)
        }
        val pkg = runCatching {
            context.packageManager.getPackageInfo(context.packageName, 0).versionName
        }.getOrNull() ?: "?"
        add(
            Level.Info, "App",
            "started — v$pkg, ${android.os.Build.MODEL}, Android ${android.os.Build.VERSION.RELEASE}",
        )
    }

    private val lineFormat = Regex("^(\\d+)\\|(I|W|E)\\|([^|]*)\\|(.*)$")

    private fun parseLine(l: String): Entry? {
        val m = lineFormat.find(l) ?: return null
        val (at, lv, tag, msg) = m.destructured
        return Entry(
            at.toLongOrNull() ?: return null,
            when (lv) { "W" -> Level.Warn; "E" -> Level.Error; else -> Level.Info },
            tag,
            msg.replace("\\u000A", "\n"),
        )
    }

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
        val entry = Entry(System.currentTimeMillis(), level, tag, clean)
        lines.addLast(entry)
        // Write-through, synchronously: the entry a crash is about to produce
        // must be on disk before the process is gone. One short line per event
        // keeps this cheap enough not to matter.
        runCatching {
            file?.appendText(
                "${entry.at}|${entry.level.name.first()}|${entry.tag}|" +
                    entry.message.replace("\n", "\\u000A") + "\n"
            )
        }
        _changes.value = _changes.value + 1
    }

    @Synchronized
    fun snapshot(): List<Entry> = lines.toList()

    @Synchronized
    fun clear() {
        lines.clear()
        runCatching { file?.writeText("") }
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
