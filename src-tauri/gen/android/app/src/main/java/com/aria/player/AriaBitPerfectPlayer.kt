package com.aria.player

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioDeviceInfo
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioMixerAttributes
import android.media.AudioTrack
import android.os.Build
import android.util.Log

class AriaBitPerfectPlayer {
    companion object {
        private const val TAG = "AriaBitPerfect"

        const val FORMAT_FAILED = 0
        const val FORMAT_PCM_16BIT = 1
        const val FORMAT_PCM_24BIT_PACKED = 2
        const val FORMAT_PCM_32BIT = 3
        const val FORMAT_PCM_FLOAT = 4

        @JvmStatic
        fun createPlayer(): AriaBitPerfectPlayer {
            return AriaBitPerfectPlayer()
        }
    }

    private var audioTrack: AudioTrack? = null
    private var audioManager: AudioManager? = null
    private var targetDevice: AudioDeviceInfo? = null
    private var audioAttributes: AudioAttributes? = null
    private var mixerAttributesConfigured: Boolean = false
    private var sampleRate: Int = 44100
    private var channelCount: Int = 2
    private var bytesPerFrame: Int = 4
    private var framesWritten: Long = 0

    fun open(
        context: Context,
        deviceId: Int,
        sampleRate: Int,
        channelCount: Int,
        bitDepth: Int
    ): Int {
        try {
            val am = context.getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: run {
                Log.e(TAG, "AudioManager not available")
                return FORMAT_FAILED
            }
            this.audioManager = am
            this.sampleRate = sampleRate
            this.channelCount = channelCount

            // Find target USB output device
            val devices = am.getDevices(AudioManager.GET_DEVICES_OUTPUTS)
            val device = if (deviceId > 0) {
                devices.find { it.id == deviceId }
            } else {
                null
            } ?: devices.find {
                it.type == AudioDeviceInfo.TYPE_USB_DEVICE || it.type == AudioDeviceInfo.TYPE_USB_HEADSET
            }

            if (device == null) {
                Log.w(TAG, "No USB audio output device found for bit-perfect routing")
                return FORMAT_FAILED
            }

            this.targetDevice = device
            Log.i(TAG, "Targeting device: ${device.productName} (id=${device.id}, type=${device.type}) for ${sampleRate}Hz, ${bitDepth}-bit, ${channelCount}ch")

            val channelMask = if (channelCount == 1) {
                AudioFormat.CHANNEL_OUT_MONO
            } else {
                AudioFormat.CHANNEL_OUT_STEREO
            }

            var chosenEncoding = when {
                bitDepth <= 16 -> AudioFormat.ENCODING_PCM_16BIT
                bitDepth <= 24 -> {
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                        AudioFormat.ENCODING_PCM_24BIT_PACKED
                    } else {
                        AudioFormat.ENCODING_PCM_FLOAT
                    }
                }
                else -> {
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                        AudioFormat.ENCODING_PCM_32BIT
                    } else {
                        AudioFormat.ENCODING_PCM_FLOAT
                    }
                }
            }

            var minBufferSize = AudioTrack.getMinBufferSize(sampleRate, channelMask, chosenEncoding)
            if (minBufferSize <= 0 && chosenEncoding == AudioFormat.ENCODING_PCM_24BIT_PACKED) {
                Log.w(TAG, "ENCODING_PCM_24BIT_PACKED not supported by AudioTrack at ${sampleRate}Hz, trying ENCODING_PCM_FLOAT")
                chosenEncoding = AudioFormat.ENCODING_PCM_FLOAT
                minBufferSize = AudioTrack.getMinBufferSize(sampleRate, channelMask, chosenEncoding)
            }
            if (minBufferSize <= 0 && chosenEncoding == AudioFormat.ENCODING_PCM_32BIT) {
                Log.w(TAG, "ENCODING_PCM_32BIT not supported by AudioTrack at ${sampleRate}Hz, trying ENCODING_PCM_FLOAT")
                chosenEncoding = AudioFormat.ENCODING_PCM_FLOAT
                minBufferSize = AudioTrack.getMinBufferSize(sampleRate, channelMask, chosenEncoding)
            }
            if (minBufferSize <= 0 && chosenEncoding == AudioFormat.ENCODING_PCM_FLOAT) {
                Log.w(TAG, "ENCODING_PCM_FLOAT not supported by AudioTrack at ${sampleRate}Hz, trying ENCODING_PCM_16BIT")
                chosenEncoding = AudioFormat.ENCODING_PCM_16BIT
                minBufferSize = AudioTrack.getMinBufferSize(sampleRate, channelMask, chosenEncoding)
            }

            val (formatCode, bytesPerSample) = when (chosenEncoding) {
                AudioFormat.ENCODING_PCM_16BIT -> Pair(FORMAT_PCM_16BIT, 2)
                AudioFormat.ENCODING_PCM_24BIT_PACKED -> Pair(FORMAT_PCM_24BIT_PACKED, 3)
                AudioFormat.ENCODING_PCM_32BIT -> Pair(FORMAT_PCM_32BIT, 4)
                AudioFormat.ENCODING_PCM_FLOAT -> Pair(FORMAT_PCM_FLOAT, 4)
                else -> Pair(FORMAT_PCM_16BIT, 2)
            }
            this.bytesPerFrame = bytesPerSample * channelCount

            // If static AudioTrack.getMinBufferSize failed (common for higher sample rates like 96k/192k on external DACs
            // because system default output only validates up to 48k), compute an explicit buffer size fallback.
            if (minBufferSize <= 0) {
                val fallbackMin = maxOf(4096, (sampleRate * bytesPerFrame * 100) / 1000)
                Log.w(TAG, "AudioTrack.getMinBufferSize returned $minBufferSize for $sampleRate Hz, using calculated fallback: $fallbackMin bytes")
                minBufferSize = fallbackMin
            }

            val audioAttrs = AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                .build()
            this.audioAttributes = audioAttrs

            val audioFormat = AudioFormat.Builder()
                .setSampleRate(sampleRate)
                .setChannelMask(channelMask)
                .setEncoding(chosenEncoding)
                .build()

            // 1. Android 14+ (API 34+): Request built-in BIT_PERFECT mixer behavior on the USB device
            if (Build.VERSION.SDK_INT >= 34) {
                try {
                    val supportedMixers = am.getSupportedMixerAttributes(device)
                    Log.i(TAG, "Device ${device.productName} supported mixer configurations: ${supportedMixers.size}")
                    for (attr in supportedMixers) {
                        val isBp = attr.mixerBehavior == AudioMixerAttributes.MIXER_BEHAVIOR_BIT_PERFECT
                        Log.d(TAG, "  Mixer attr: format=${attr.format}, behavior=${attr.mixerBehavior} (isBitPerfect=$isBp)")
                    }

                    // Prefer an exact matching bit-perfect attribute from the HAL, or construct one
                    val bitPerfectAttr = supportedMixers.find { attr ->
                        attr.mixerBehavior == AudioMixerAttributes.MIXER_BEHAVIOR_BIT_PERFECT &&
                            attr.format.sampleRate == sampleRate &&
                            attr.format.channelMask == channelMask &&
                            attr.format.encoding == chosenEncoding
                    } ?: supportedMixers.find { attr ->
                        attr.mixerBehavior == AudioMixerAttributes.MIXER_BEHAVIOR_BIT_PERFECT &&
                            attr.format.sampleRate == sampleRate
                    } ?: supportedMixers.find { attr ->
                        attr.mixerBehavior == AudioMixerAttributes.MIXER_BEHAVIOR_BIT_PERFECT
                    } ?: AudioMixerAttributes.Builder(audioFormat)
                        .setMixerBehavior(AudioMixerAttributes.MIXER_BEHAVIOR_BIT_PERFECT)
                        .build()

                    val setOk = am.setPreferredMixerAttributes(
                        audioAttrs,
                        device,
                        bitPerfectAttr
                    )
                    if (setOk) {
                        mixerAttributesConfigured = true
                        Log.i(TAG, "Successfully enabled built-in MIXER_BEHAVIOR_BIT_PERFECT on ${device.productName} at ${sampleRate}Hz")
                    } else {
                        Log.w(TAG, "setPreferredMixerAttributes returned false for ${device.productName}")
                    }
                } catch (e: Exception) {
                    Log.w(TAG, "Failed to set preferred mixer attributes: ${e.message}")
                }
            }

            // 2. Build AudioTrack matching the exact native stream parameters
            val targetBufferSize = maxOf(minBufferSize * 4, sampleRate * bytesPerFrame / 5) // at least 200ms

            val builder = AudioTrack.Builder()
                .setAudioAttributes(audioAttrs)
                .setAudioFormat(audioFormat)
                .setBufferSizeInBytes(targetBufferSize)
                .setTransferMode(AudioTrack.MODE_STREAM)

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                builder.setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
            }

            val track = builder.build()
            track.preferredDevice = device

            if (track.state != AudioTrack.STATE_INITIALIZED) {
                Log.e(TAG, "AudioTrack failed to initialize (state=${track.state})")
                track.release()
                return FORMAT_FAILED
            }

            this.audioTrack = track
            this.framesWritten = 0
            Log.i(TAG, "AudioTrack initialized successfully in bit-accurate mode (encoding=$chosenEncoding, buffer size=$targetBufferSize bytes)")
            return formatCode
        } catch (e: Exception) {
            Log.e(TAG, "Error opening bit-accurate audio track", e)
            return FORMAT_FAILED
        }
    }

    fun write(data: ByteArray, offset: Int, size: Int): Int {
        val track = audioTrack ?: return -1
        return try {
            val written = track.write(data, offset, size, AudioTrack.WRITE_BLOCKING)
            if (written > 0 && bytesPerFrame > 0) {
                framesWritten += written / bytesPerFrame
            }
            written
        } catch (e: Exception) {
            Log.e(TAG, "Error writing audio data", e)
            -1
        }
    }

    fun play(): Boolean {
        val track = audioTrack ?: return false
        return try {
            track.play()
            true
        } catch (e: Exception) {
            Log.e(TAG, "Error starting playback", e)
            false
        }
    }

    fun pause(): Boolean {
        val track = audioTrack ?: return false
        return try {
            track.pause()
            true
        } catch (e: Exception) {
            Log.e(TAG, "Error pausing playback", e)
            false
        }
    }

    fun flush(): Boolean {
        val track = audioTrack ?: return false
        return try {
            track.flush()
            true
        } catch (e: Exception) {
            Log.e(TAG, "Error flushing audio track", e)
            false
        }
    }

    fun getPositionMs(): Long {
        val track = audioTrack ?: return 0L
        return try {
            val head = track.playbackHeadPosition.toLong() and 0xFFFFFFFFL
            if (sampleRate > 0) {
                (head * 1000L) / sampleRate
            } else {
                0L
            }
        } catch (e: Exception) {
            0L
        }
    }

    fun close(): Boolean {
        return try {
            audioTrack?.let { track ->
                try {
                    track.stop()
                } catch (_: Exception) {}
                track.release()
            }
            audioTrack = null

            if (Build.VERSION.SDK_INT >= 34 && mixerAttributesConfigured) {
                val am = audioManager
                val dev = targetDevice
                val attrs = audioAttributes
                if (am != null && dev != null && attrs != null) {
                    try {
                        am.clearPreferredMixerAttributes(attrs, dev)
                        Log.i(TAG, "Cleared preferred mixer attributes for ${dev.productName}")
                    } catch (e: Exception) {
                        Log.w(TAG, "Failed to clear preferred mixer attributes", e)
                    }
                }
                mixerAttributesConfigured = false
            }

            true
        } catch (e: Exception) {
            Log.e(TAG, "Error closing bit-accurate audio track", e)
            false
        }
    }
}
