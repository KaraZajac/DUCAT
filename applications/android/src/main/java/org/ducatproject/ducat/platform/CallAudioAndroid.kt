package org.ducatproject.ducat.platform

import android.annotation.SuppressLint
import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.AudioTrack
import android.media.MediaRecorder
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import org.ducatproject.ducat.Calls
import org.ducatproject.ducat.DucatLog

/**
 * The phone's ears and mouth for §16.21: VOICE_COMMUNICATION capture (the
 * platform's echo-cancelled, noise-suppressed path) into 20 ms PCM16
 * frames, and a VOICE_COMMUNICATION track back out — the earpiece route a
 * phone call is expected to use, not the media speaker.
 *
 * Installed by MainActivity the way DeviceLock's backend is, because the
 * shared sources cannot name android.media.
 */
object CallAudioAndroid : Calls.Audio {
    private const val RATE = 16_000
    @Volatile private var recorder: AudioRecord? = null
    @Volatile private var track: AudioTrack? = null
    @Volatile private var capturing = false

    @SuppressLint("MissingPermission") // Callers gate on RECORD_AUDIO first.
    @Synchronized
    override fun start(onFrame: (ByteArray) -> Unit): Boolean {
        stop()
        val minRec = AudioRecord.getMinBufferSize(
            RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT,
        )
        val rec = runCatching {
            AudioRecord(
                MediaRecorder.AudioSource.VOICE_COMMUNICATION,
                RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT,
                maxOf(minRec, Calls.FRAME_BYTES * 4),
            )
        }.getOrNull()
        if (rec == null || rec.state != AudioRecord.STATE_INITIALIZED) {
            DucatLog.w("CallAudio", "microphone would not open")
            rec?.release()
            return false
        }
        val minPlay = AudioTrack.getMinBufferSize(
            RATE, AudioFormat.CHANNEL_OUT_MONO, AudioFormat.ENCODING_PCM_16BIT,
        )
        val out = runCatching {
            AudioTrack.Builder()
                .setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                        .build(),
                )
                .setAudioFormat(
                    AudioFormat.Builder()
                        .setSampleRate(RATE)
                        .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                        .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                        .build(),
                )
                .setBufferSizeInBytes(maxOf(minPlay, Calls.FRAME_BYTES * 6))
                .setTransferMode(AudioTrack.MODE_STREAM)
                .build()
        }.getOrElse {
            DucatLog.w("CallAudio", "earpiece would not open: ${it.message}")
            rec.release()
            return false
        }
        recorder = rec
        track = out
        capturing = true
        runCatching { rec.startRecording(); out.play() }.onFailure {
            DucatLog.w("CallAudio", "microphone would not start: ${it.message}")
            stop()
            return false
        }
        Thread {
            val buf = ByteArray(Calls.FRAME_BYTES)
            var filled = 0
            // THIS recorder, not the field: stop() releases it and the next
            // start() installs another, and a read on a released recorder
            // fails at once, for ever — a loop that took an error for
            // "nothing yet" spun a core until the process died.
            while (capturing && recorder === rec) {
                val n = rec.read(buf, filled, buf.size - filled)
                if (n < 0) {
                    DucatLog.w("CallAudio", "microphone read failed ($n)")
                    break
                }
                if (n == 0) continue
                filled += n
                if (filled == buf.size) {
                    onFrame(buf.copyOf())
                    filled = 0
                }
            }
        }.apply { isDaemon = true; name = "call-mic"; start() }
        return true
    }

    override fun play(frame: ByteArray) {
        // The track can be released under this write: the ear thread plays
        // and the hang-up releases, and they are not the same thread. A
        // released track throws, and the throw was landing on the ear
        // thread — which is also the call's watchdog, so a call could be
        // left up, deaf, with nothing left running to notice.
        runCatching { track?.write(frame, 0, frame.size) }
    }

    @Synchronized
    override fun stop() {
        capturing = false
        recorder?.let { runCatching { it.stop() }; it.release() }
        track?.let { runCatching { it.stop() }; it.release() }
        recorder = null
        track = null
    }

    // ----- the bell: the British ring-ring, synthesized, no asset -----

    @Volatile private var bell: AudioTrack? = null
    @Volatile private var buzzer: Vibrator? = null

    /**
     * **Nothing in here may throw.** The bell is the one part of a call
     * that is decoration, and it was the part that could take the call
     * down: `place` rings before it dials and `startRinging` rings before
     * it shows the ask, neither behind a catch, so an AudioTrack that will
     * not build — an unusual audio HAL, a ringtone stream held by
     * something else — crashed the app at the exact moment somebody made
     * or received a call. Synchronized with [quiet] for the same reason
     * [start] and [stop] are: two rings racing leave one track playing
     * with nothing left holding it.
     *
     * A phone that cannot ring still connects. That is the contract the
     * interface states ("hosts without a bell inherit silence") and this
     * is where it has to be kept.
     */
    @Synchronized
    override fun ring(context: Context, incoming: Boolean) {
        runCatching { rings(context, incoming) }
            .onFailure { DucatLog.w("CallAudio", "no bell: ${it.message}") }
    }

    private fun rings(context: Context, incoming: Boolean) {
        quiet()
        val app = context.applicationContext
        val am = app.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        if (incoming) {
            // A ringing phone owes the room the user's own rules.
            if (am.ringerMode != AudioManager.RINGER_MODE_SILENT) buzz(app)
            if (am.ringerMode != AudioManager.RINGER_MODE_NORMAL) {
                DucatLog.i("CallAudio", "ring: mode=${am.ringerMode}, bell held")
                return
            }
        }
        val pcm = Calls.ukRing(if (incoming) 0.5 else 0.18)
        val usage = if (incoming) {
            AudioAttributes.USAGE_NOTIFICATION_RINGTONE
        } else {
            AudioAttributes.USAGE_VOICE_COMMUNICATION
        }
        val t = AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(usage)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                    .build(),
            )
            .setAudioFormat(
                AudioFormat.Builder()
                    .setSampleRate(RATE)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                    .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                    .build(),
            )
            .setBufferSizeInBytes(pcm.size)
            .setTransferMode(AudioTrack.MODE_STATIC)
            .build()
        t.write(pcm, 0, pcm.size)
        t.setLoopPoints(0, pcm.size / 2, -1)
        t.play()
        bell = t
        DucatLog.i("CallAudio", "ring: ${if (incoming) "bell" else "ringback"} on")
    }

    @Synchronized
    override fun quiet() {
        bell?.let { runCatching { it.stop() }; runCatching { it.release() } }
        bell = null
        runCatching { buzzer?.cancel() }
        buzzer = null
    }

    /** The same cadence the bell has, felt: ring-ring, rest, again. */
    private fun buzz(app: Context) {
        val v = if (Build.VERSION.SDK_INT >= 31) {
            (app.getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as VibratorManager)
                .defaultVibrator
        } else {
            @Suppress("DEPRECATION")
            app.getSystemService(Context.VIBRATOR_SERVICE) as Vibrator
        }
        // Held before it is started: the waveform repeats until something
        // cancels it, and a quiet() that arrived between the two lines left
        // a phone buzzing with nothing on screen and no handle on it.
        buzzer = v
        v.vibrate(VibrationEffect.createWaveform(longArrayOf(0, 400, 200, 400, 2_000), 0))
    }
}
