package com.aria.player

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioDeviceInfo
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioMixerAttributes
import android.media.AudioTrack
import android.net.wifi.WifiManager
import android.os.Build
import android.os.PowerManager
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
    private var appContext: Context? = null
    private var wakeLock: PowerManager.WakeLock? = null
    private var wifiLock: WifiManager.WifiLock? = null
    private var currentTitle: String = "Aria"
    private var currentArtist: String = "Playing audio"

    fun setTrackMetadata(title: String?, artist: String?) {
        if (!title.isNullOrBlank()) this.currentTitle = title
        if (!artist.isNullOrBlank()) this.currentArtist = artist
        appContext?.let {
            val isPlaying = audioTrack?.playState == AudioTrack.PLAYSTATE_PLAYING
            AriaPlaybackService.updatePlayback(it, this.currentTitle, this.currentArtist, isPlaying)
        }
    }

    private fun acquireLocks() {
        val ctx = appContext ?: return
        try {
            if (wakeLock == null) {
                val pm = ctx.getSystemService(Context.POWER_SERVICE) as? PowerManager
                wakeLock = pm?.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "Aria:BitPerfectPlayback")?.apply {
                    setReferenceCounted(false)
                }
            }
            wakeLock?.let {
                if (!it.isHeld) {
                    it.acquire(45 * 60 * 1000L) // 45 min safety timeout
                }
            }
        } catch (e: Exception) {
            Log.w(TAG, "Could not acquire WakeLock", e)
        }

        try {
            if (wifiLock == null) {
                val wm = ctx.applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
                wifiLock = wm?.createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "Aria:PlaybackWifiLock")?.apply {
                    setReferenceCounted(false)
                }
            }
            wifiLock?.let {
                if (!it.isHeld) {
                    it.acquire()
                }
            }
        } catch (e: Exception) {
            Log.w(TAG, "Could not acquire WifiLock", e)
        }
    }

    private fun releaseLocks() {
        try {
            wakeLock?.let {
                if (it.isHeld) it.release()
            }
            wakeLock = null
        } catch (e: Exception) {
            Log.w(TAG, "Error releasing WakeLock", e)
        }

        try {
            wifiLock?.let {
                if (it.isHeld) it.release()
            }
            wifiLock = null
        } catch (e: Exception) {
            Log.w(TAG, "Error releasing WifiLock", e)
        }
    }

    private fun isScoDevice(device: AudioDeviceInfo): Boolean {
        if (device.type == AudioDeviceInfo.TYPE_BLUETOOTH_SCO) return true
        val name = device.productName?.toString()?.lowercase() ?: ""
        return name.contains("sco") || name.contains("handsfree") || name.contains("telephony")
    }

    fun open(
        context: Context,
        deviceId: Int,
        sampleRate: Int,
        channelCount: Int,
        bitDepth: Int
    ): Int {
        try {
            this.appContext = context.applicationContext
            val am = context.getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: run {
                Log.e(TAG, "AudioManager not available")
                return FORMAT_FAILED
            }
            this.audioManager = am
            this.sampleRate = sampleRate
            this.channelCount = channelCount

            // Find target output device:
            // 1. Explicit deviceId if provided (> 0) and not an SCO voice profile
            // 2. USB device fallback (external DAC)
            // 3. Bluetooth A2DP device fallback (HD audio headphones/earbuds)
            val devices = am.getDevices(AudioManager.GET_DEVICES_OUTPUTS)
            val usbDevice = devices.find {
                it.type == AudioDeviceInfo.TYPE_USB_DEVICE || it.type == AudioDeviceInfo.TYPE_USB_HEADSET
            }
            val a2dpDevice = devices.find {
                AriaBluetoothAudioHelper.isBluetoothDevice(it) && !isScoDevice(it)
            }
            val defaultDevice = devices.find {
                it.type == AudioDeviceInfo.TYPE_WIRED_HEADSET ||
                    it.type == AudioDeviceInfo.TYPE_WIRED_HEADPHONES ||
                    it.type == AudioDeviceInfo.TYPE_BUILTIN_SPEAKER ||
                    it.type == AudioDeviceInfo.TYPE_LINE_ANALOG ||
                    it.type == AudioDeviceInfo.TYPE_LINE_DIGITAL
            } ?: devices.firstOrNull()

            val device = if (deviceId > 0) {
                devices.find { it.id == deviceId && !isScoDevice(it) }
            } else {
                null
            } ?: usbDevice ?: a2dpDevice ?: defaultDevice

            if (device == null) {
                Log.w(TAG, "No suitable audio output device found for high-res routing")
                return FORMAT_FAILED
            }

            val isBluetooth = AriaBluetoothAudioHelper.isBluetoothDevice(device)
            if (isBluetooth) {
                Log.i(TAG, "Bluetooth output device targeted: ${device.productName} (id=${device.id}). Configuring for native source rate ${sampleRate}Hz (NEVER UPSAMPLING)...")
                val btInfo = AriaBluetoothAudioHelper.optimizeForHighestQuality(context, sampleRate)
                Log.i(TAG, "Bluetooth HD audio active: ${btInfo.summary}")
            }

            this.targetDevice = device
            Log.i(TAG, "Targeting device: ${device.productName} (id=${device.id}, type=${device.type}, isBluetooth=$isBluetooth) for ${sampleRate}Hz, ${bitDepth}-bit, ${channelCount}ch")

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

            // 1. Android 14+ (API 34+): Request built-in BIT_PERFECT mixer behavior on external USB DAC devices
            if (Build.VERSION.SDK_INT >= 34 && !isBluetooth) {
                try {
                    val supportedMixers = am.getSupportedMixerAttributes(device)
                    Log.i(TAG, "Device ${device.productName} supported mixer configurations: ${supportedMixers.size}")
                    for (attr in supportedMixers) {
                        val isBp = attr.mixerBehavior == AudioMixerAttributes.MIXER_BEHAVIOR_BIT_PERFECT
                        Log.d(TAG, "  Mixer attr: format=${attr.format}, behavior=${attr.mixerBehavior} (isBitPerfect=$isBp)")
                    }

                    // Prefer an exact matching bit-perfect attribute at native source sampleRate from the HAL, or construct one.
                    // CRITICAL AUDIOPHILE RULE: NEVER select a mixer attribute with a different/higher sample rate, as that forces up-sampling!
                    val bitPerfectAttr = supportedMixers.find { attr ->
                        attr.mixerBehavior == AudioMixerAttributes.MIXER_BEHAVIOR_BIT_PERFECT &&
                            attr.format.sampleRate == sampleRate &&
                            attr.format.channelMask == channelMask &&
                            attr.format.encoding == chosenEncoding
                    } ?: supportedMixers.find { attr ->
                        attr.mixerBehavior == AudioMixerAttributes.MIXER_BEHAVIOR_BIT_PERFECT &&
                            attr.format.sampleRate == sampleRate
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
                        Log.i(TAG, "Successfully enabled built-in MIXER_BEHAVIOR_BIT_PERFECT on ${device.productName} at native rate ${sampleRate}Hz")
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
                if (isBluetooth) {
                    builder.setPerformanceMode(AudioTrack.PERFORMANCE_MODE_NONE)
                } else {
                    builder.setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
                }
            }

            val track = builder.build()
            if (device != null && !isBluetooth) {
                track.preferredDevice = device
            }

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
            acquireLocks()
            appContext?.let {
                AriaPlaybackService.startPlayback(it, currentTitle, currentArtist)
            }
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
            releaseLocks()
            appContext?.let {
                AriaPlaybackService.updatePlayback(it, currentTitle, currentArtist, false)
            }
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
            framesWritten = 0
            true
        } catch (e: Exception) {
            Log.e(TAG, "Error flushing audio track", e)
            false
        }
    }

    fun endOfStream(): Boolean {
        val track = audioTrack ?: return true
        return try {
            if (track.playState == AudioTrack.PLAYSTATE_PLAYING) {
                track.stop()
            }
            true
        } catch (e: Exception) {
            Log.e(TAG, "Error initiating end of stream drain", e)
            false
        }
    }

    fun isFinished(): Boolean {
        val track = audioTrack ?: return true
        return try {
            if (track.playState == AudioTrack.PLAYSTATE_STOPPED) {
                return true
            }
            if (framesWritten <= 0) {
                return true
            }
            val head = track.playbackHeadPosition.toLong() and 0xFFFFFFFFL
            head >= framesWritten
        } catch (e: Exception) {
            true
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
        releaseLocks()
        appContext?.let {
            AriaPlaybackService.stopPlayback(it)
        }
        return try {
            audioTrack?.let { track ->
                try {
                    track.pause()
                    track.flush()
                } catch (_: Exception) {}
                try {
                    track.stop()
                } catch (_: Exception) {}
                track.release()
            }
            audioTrack = null
            framesWritten = 0

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
