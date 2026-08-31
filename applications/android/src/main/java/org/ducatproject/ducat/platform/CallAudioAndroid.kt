package org.ducatproject.ducat.platform

import android.annotation.SuppressLint
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.AudioTrack
import android.media.MediaRecorder
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
    override fun start(onFrame: (ByteArray) -> Unit) {
        stop()
        val minRec = AudioRecord.getMinBufferSize(
            RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT,
        )
        val rec = AudioRecord(
            MediaRecorder.AudioSource.VOICE_COMMUNICATION,
            RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT,
            maxOf(minRec, Calls.FRAME_BYTES * 4),
        )
        if (rec.state != AudioRecord.STATE_INITIALIZED) {
            DucatLog.w("CallAudio", "microphone would not open")
            rec.release()
            return
        }
        val minPlay = AudioTrack.getMinBufferSize(
            RATE, AudioFormat.CHANNEL_OUT_MONO, AudioFormat.ENCODING_PCM_16BIT,
        )
        val out = AudioTrack.Builder()
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
        recorder = rec
        track = out
        capturing = true
        rec.startRecording()
        out.play()
        Thread {
            val buf = ByteArray(Calls.FRAME_BYTES)
            var filled = 0
            while (capturing) {
                val n = rec.read(buf, filled, buf.size - filled)
                if (n <= 0) continue
                filled += n
                if (filled == buf.size) {
                    onFrame(buf.copyOf())
                    filled = 0
                }
            }
        }.apply { isDaemon = true; name = "call-mic"; start() }
    }

    override fun play(frame: ByteArray) {
        track?.write(frame, 0, frame.size)
    }

    override fun stop() {
        capturing = false
        recorder?.let { runCatching { it.stop() }; it.release() }
        track?.let { runCatching { it.stop() }; it.release() }
        recorder = null
        track = null
    }
}
