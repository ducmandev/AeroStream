package com.aerostream.android_app

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.media.MediaCodec
import android.media.MediaFormat
import android.os.Build
import android.util.Log
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.BasicMessageChannel
import io.flutter.plugin.common.BinaryCodec
import io.flutter.plugin.common.MethodChannel
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

class MainActivity : FlutterActivity() {
    companion object {
        private const val TAG = "AeroStreamAudio"
        private const val CONTROL_CHANNEL = "aerostream/audio_control"
        private const val STREAM_CHANNEL = "aerostream/audio_stream"
        private const val SAMPLE_RATE = 48000
        private const val CHANNELS = 2
        // Adaptive low-latency queue: max 16 packets (~320ms buffer).
        // Drops oldest packet only under severe network backpressure.
        private const val MAX_QUEUE_CAPACITY = 16
    }

    private val isRunning = AtomicBoolean(false)
    private var isMuted = false
    private val packetQueue = ArrayBlockingQueue<ByteArray>(MAX_QUEUE_CAPACITY)
    
    private var audioTrack: AudioTrack? = null
    private var mediaCodec: MediaCodec? = null
    private var audioThread: Thread? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)

        // Control channel for start, stop, mute commands
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, CONTROL_CHANNEL).setMethodCallHandler { call, result ->
            when (call.method) {
                "start" -> {
                    startAudio()
                    result.success(true)
                }
                "stop" -> {
                    stopAudio()
                    result.success(true)
                }
                "setMuted" -> {
                    val muted = call.argument<Boolean>("muted") ?: false
                    isMuted = muted
                    if (muted) {
                        packetQueue.clear()
                    }
                    result.success(true)
                }
                else -> result.notImplemented()
            }
        }

        // High-performance binary channel for streaming Opus frames without JSON serialization
        val binaryChannel = BasicMessageChannel(
            flutterEngine.dartExecutor.binaryMessenger,
            STREAM_CHANNEL,
            BinaryCodec.INSTANCE
        )
        binaryChannel.setMessageHandler { message, reply ->
            if (message != null && isRunning.get() && !isMuted) {
                val size = message.remaining()
                if (size > 0) {
                    val bytes = ByteArray(size)
                    message.get(bytes)
                    // Drop-on-lag policy: if queue is full, drop the oldest packet
                    // to prevent audio delay accumulation
                    if (!packetQueue.offer(bytes)) {
                        packetQueue.poll()
                        packetQueue.offer(bytes)
                    }
                }
            }
            reply.reply(null)
        }
    }

    @Synchronized
    private fun startAudio() {
        if (isRunning.get()) return
        packetQueue.clear()
        isRunning.set(true)

        audioThread = Thread({
            android.os.Process.setThreadPriority(android.os.Process.THREAD_PRIORITY_URGENT_AUDIO)
            try {
                initCodecAndTrack()
                decodeAndPlayLoop()
            } catch (e: Exception) {
                Log.e(TAG, "Audio loop error: ${e.message}", e)
            } finally {
                isRunning.set(false)
                releaseCodecAndTrack()
            }
        }, "AeroStream-AudioWorker").apply {
            start()
        }
        Log.i(TAG, "Native low-latency Opus audio player started")
    }

    @Synchronized
    private fun stopAudio() {
        if (!isRunning.get()) return
        isRunning.set(false)
        packetQueue.clear()
        audioThread?.interrupt()
        try {
            audioThread?.join(500)
        } catch (_: InterruptedException) {}
        audioThread = null
        Log.i(TAG, "Native audio player stopped")
    }

    private fun initCodecAndTrack() {
        // 1. Configure MediaCodec for Opus decoding
        val format = MediaFormat.createAudioFormat(MediaFormat.MIMETYPE_AUDIO_OPUS, SAMPLE_RATE, CHANNELS)
        format.setInteger(MediaFormat.KEY_SAMPLE_RATE, SAMPLE_RATE)
        format.setInteger(MediaFormat.KEY_CHANNEL_COUNT, CHANNELS)
        
        // CSD-0: OpusHead identification header (19 bytes)
        val csd0 = ByteBuffer.allocate(19).order(ByteOrder.LITTLE_ENDIAN)
        csd0.put("OpusHead".toByteArray(Charsets.US_ASCII)) // 8 bytes magic
        csd0.put(1.toByte())                                 // Version 1
        csd0.put(CHANNELS.toByte())                          // Channels = 2
        csd0.putShort(312.toShort())                         // Pre-skip = 312
        csd0.putInt(SAMPLE_RATE)                             // 48000 Hz
        csd0.putShort(0.toShort())                           // Gain = 0
        csd0.put(0.toByte())                                 // Channel mapping = 0
        csd0.flip()
        format.setByteBuffer("csd-0", csd0)

        // CSD-1: Codec delay in nanoseconds (native byte order)
        val delayNs = (312L * 1000000000L) / SAMPLE_RATE
        val csd1 = ByteBuffer.allocate(8).order(ByteOrder.nativeOrder()).putLong(delayNs)
        csd1.flip()
        format.setByteBuffer("csd-1", csd1)

        // CSD-2: Seek preroll in nanoseconds (80ms standard, native byte order)
        val csd2 = ByteBuffer.allocate(8).order(ByteOrder.nativeOrder()).putLong(80000000L)
        csd2.flip()
        format.setByteBuffer("csd-2", csd2)

        val codec = MediaCodec.createDecoderByType(MediaFormat.MIMETYPE_AUDIO_OPUS)
        codec.configure(format, null, null, 0)
        codec.start()
        mediaCodec = codec

        // 2. Configure AudioTrack for low-latency PCM playback
        val minBuf = AudioTrack.getMinBufferSize(
            SAMPLE_RATE,
            AudioFormat.CHANNEL_OUT_STEREO,
            AudioFormat.ENCODING_PCM_16BIT
        )
        val track = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            AudioTrack.Builder()
                .setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_MEDIA)
                        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                        .build()
                )
                .setAudioFormat(
                    AudioFormat.Builder()
                        .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                        .setSampleRate(SAMPLE_RATE)
                        .setChannelMask(AudioFormat.CHANNEL_OUT_STEREO)
                        .build()
                )
                .setBufferSizeInBytes(minBuf.coerceAtLeast(4096) * 2)
                .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
                .build()
        } else {
            @Suppress("DEPRECATION")
            AudioTrack(
                android.media.AudioManager.STREAM_MUSIC,
                SAMPLE_RATE,
                AudioFormat.CHANNEL_OUT_STEREO,
                AudioFormat.ENCODING_PCM_16BIT,
                minBuf.coerceAtLeast(4096) * 2,
                AudioTrack.MODE_STREAM
            )
        }

        if (track.state != AudioTrack.STATE_INITIALIZED) {
            Log.e(TAG, "AudioTrack failed to initialize! state=${track.state}")
            throw IllegalStateException("AudioTrack not initialized")
        }

        track.play()
        audioTrack = track
        Log.i(TAG, "Opus MediaCodec and AudioTrack initialized successfully (48kHz stereo)")
    }

    private fun decodeAndPlayLoop() {
        val codec = mediaCodec ?: return
        val track = audioTrack ?: return
        val bufferInfo = MediaCodec.BufferInfo()
        var ptsUs = 0L
        var pendingPacket: ByteArray? = null

        while (isRunning.get() && !Thread.currentThread().isInterrupted) {
            if (pendingPacket == null) {
                pendingPacket = packetQueue.poll(10, TimeUnit.MILLISECONDS)
            }

            if (pendingPacket != null) {
                val inIndex = codec.dequeueInputBuffer(5000)
                if (inIndex >= 0) {
                    val inBuf = codec.getInputBuffer(inIndex)
                    if (inBuf != null) {
                        inBuf.clear()
                        inBuf.put(pendingPacket)
                        codec.queueInputBuffer(inIndex, 0, pendingPacket.size, ptsUs, 0)
                        ptsUs += 20000L // 20ms per Opus frame
                        pendingPacket = null // successfully queued
                    }
                }
            }

            // Drain all available output decoded PCM buffers
            var outIndex = codec.dequeueOutputBuffer(bufferInfo, 2000)
            while (outIndex >= 0 || outIndex == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                if (outIndex == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                    Log.i(TAG, "MediaCodec output format changed: ${codec.outputFormat}")
                } else if (outIndex >= 0) {
                    val outBuf = codec.getOutputBuffer(outIndex)
                    if (outBuf != null && bufferInfo.size > 0 && !isMuted) {
                        outBuf.position(bufferInfo.offset)
                        outBuf.limit(bufferInfo.offset + bufferInfo.size)
                        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
                            track.write(outBuf, bufferInfo.size, AudioTrack.WRITE_BLOCKING)
                        } else {
                            val pcm = ByteArray(bufferInfo.size)
                            outBuf.get(pcm)
                            track.write(pcm, 0, pcm.size)
                        }
                    }
                    codec.releaseOutputBuffer(outIndex, false)
                }
                outIndex = codec.dequeueOutputBuffer(bufferInfo, 0)
            }
        }
    }

    private fun releaseCodecAndTrack() {
        try {
            audioTrack?.apply {
                stop()
                release()
            }
        } catch (_: Exception) {}
        audioTrack = null

        try {
            mediaCodec?.apply {
                stop()
                release()
            }
        } catch (_: Exception) {}
        mediaCodec = null
    }

    override fun onPause() {
        super.onPause()
        stopAudio()
    }

    override fun onDestroy() {
        super.onDestroy()
        stopAudio()
    }
}
