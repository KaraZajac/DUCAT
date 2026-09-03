// Voice memos on a machine with a microphone and no phone codecs.
//
// The phone records AAC-in-MP4 because every phone decodes it. A JVM does
// not: java.sound reads WAV, AU and AIFF and nothing else without a native
// codec. So the desk records **WAV** — larger per second, but universally
// decodable, and Android's own MediaPlayer plays it — and labels it wav
// rather than pretending to be the phone's m4a (see sendVoice, which now
// reads the extension instead of assuming).
//
// The reverse direction is the honest limit: an m4a recorded on a phone
// cannot be decoded here, and the player says so rather than failing mute.

package android.media

import javax.sound.sampled.AudioFileFormat
import javax.sound.sampled.AudioFormat
import javax.sound.sampled.AudioInputStream
import javax.sound.sampled.AudioSystem
import javax.sound.sampled.DataLine
import javax.sound.sampled.TargetDataLine

class MediaRecorder {
    object AudioSource { const val MIC = 1 }
    object OutputFormat { const val MPEG_4 = 2; const val THREE_GPP = 1 }
    object AudioEncoder { const val AAC = 3; const val AMR_NB = 1 }

    private var line: TargetDataLine? = null
    private var target: java.io.File? = null
    private var worker: Thread? = null

    fun setAudioSource(source: Int) {}
    fun setOutputFormat(format: Int) {}
    fun setAudioEncoder(encoder: Int) {}
    fun setAudioEncodingBitRate(rate: Int) {}
    fun setAudioSamplingRate(rate: Int) {}

    fun setOutputFile(path: String) { target = java.io.File(path) }

    fun prepare() {
        // 16 kHz mono is speech-shaped: a minute lands near 1.9 MB, inside one
        // record, and every decoder on either side reads it.
        val fmt = AudioFormat(16_000f, 16, 1, true, false)
        val info = DataLine.Info(TargetDataLine::class.java, fmt)
        check(AudioSystem.isLineSupported(info)) { "no microphone available" }
        line = (AudioSystem.getLine(info) as TargetDataLine).apply { open(fmt) }
    }

    fun start() {
        val l = line ?: error("not prepared")
        val f = target ?: error("no output file")
        l.start()
        worker = Thread {
            runCatching {
                AudioSystem.write(AudioInputStream(l), AudioFileFormat.Type.WAVE, f)
            }
        }.also { it.start() }
    }

    fun stop() {
        line?.stop()
        line?.close()
        // AudioSystem.write returns when the line closes; give it that moment
        // so the header is finalised before anyone reads the file.
        worker?.join(2_000)
        worker = null
    }

    fun release() {
        runCatching { line?.close() }
        line = null
    }
}

class MediaPlayer {
    private var clip: javax.sound.sampled.Clip? = null
    private var onComplete: ((MediaPlayer) -> Unit)? = null
    private var source: java.io.File? = null

    fun reset() {
        runCatching { clip?.stop(); clip?.close() }
        clip = null
    }

    fun setDataSource(path: String) { source = java.io.File(path) }

    fun setOnCompletionListener(l: (MediaPlayer) -> Unit) { onComplete = l }

    private var onError: ((MediaPlayer, Int, Int) -> Boolean)? = null
    private var onPrepared: ((MediaPlayer) -> Unit)? = null

    fun setOnErrorListener(l: (MediaPlayer, Int, Int) -> Boolean) { onError = l }
    fun setOnPreparedListener(l: (MediaPlayer) -> Unit) { onPrepared = l }

    /**
     * [prepare] off the calling thread, the listeners told on the UI one —
     * the order Android keeps: the caller's own block finishes before either
     * callback runs, so state it sets after asking (a "playing" flag the
     * prepared listener reads) is in place when the answer comes.
     */
    fun prepareAsync() {
        Thread {
            val r = runCatching { prepare() }
            javax.swing.SwingUtilities.invokeLater {
                r.onSuccess { onPrepared?.invoke(this) }
                    .onFailure { onError?.invoke(this, 1, 0) }
            }
        }.apply { isDaemon = true }.start()
    }

    fun prepare() {
        val f = source ?: error("no source")
        val stream = runCatching { AudioSystem.getAudioInputStream(f) }.getOrElse {
            // An m4a from a phone: no JVM decoder exists for it. Say which,
            // rather than leaving a play button that does nothing.
            android.widget.Toast.makeText(
                null,
                "That voice memo was recorded on a phone (m4a); this desk has " +
                    "no decoder for it.",
                android.widget.Toast.LENGTH_LONG,
            ).show()
            throw it
        }
        clip = AudioSystem.getClip().apply {
            open(stream)
            addLineListener { e ->
                if (e.type == javax.sound.sampled.LineEvent.Type.STOP) {
                    onComplete?.invoke(this@MediaPlayer)
                }
            }
        }
    }

    fun start() { clip?.start() }
    fun stop() { runCatching { clip?.stop() } }
    fun release() { runCatching { clip?.close() }; clip = null }
}

/**
 * android.media.ExifInterface, enough for SafeImage.stripped to ask the
 * one question it asks.
 *
 * Always upright. The desk decodes through ImageIO, which — like
 * BitmapFactory on the phone — ignores EXIF orientation, so there is no
 * tag being honoured here that dropping the metadata would contradict.
 * A desk that grew a real reader would fix nothing that is currently
 * wrong; it would only start rotating pictures it does not rotate today.
 */
class ExifInterface(@Suppress("UNUSED_PARAMETER") stream: java.io.InputStream) {
    fun getAttributeInt(@Suppress("UNUSED_PARAMETER") tag: String, fallback: Int): Int = fallback

    companion object {
        const val TAG_ORIENTATION = "Orientation"
        const val ORIENTATION_NORMAL = 1
        const val ORIENTATION_FLIP_HORIZONTAL = 2
        const val ORIENTATION_ROTATE_180 = 3
        const val ORIENTATION_FLIP_VERTICAL = 4
        const val ORIENTATION_TRANSPOSE = 5
        const val ORIENTATION_ROTATE_90 = 6
        const val ORIENTATION_TRANSVERSE = 7
        const val ORIENTATION_ROTATE_270 = 8
    }
}
