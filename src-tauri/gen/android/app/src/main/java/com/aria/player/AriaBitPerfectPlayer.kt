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
import java.nio.ByteBuffer
import java.nio.ByteOrder

class AriaBitPerfectPlayer {
    companion object {
        private const val TAG = "AriaBitPerfect"

        const val FORMAT_FAILED = 0
        const val FORMAT_PCM_16BIT = 1
        const val FORMAT_PCM_24BIT_PACKED = 2
        const val FORMAT_PCM_32BIT = 3
        const val FORMAT_PCM_FLOAT = 4

        @Volatile
        var activeInstance: AriaBitPerfectPlayer? = null
            private set

        @JvmStatic
        fun silenceActivePlayback() {
            try {
                activeInstance?.let { player ->
                    player.audioTrack?.let { track ->
                        try {
                            track.pause()
                            track.flush()
                        } catch (_: Exception) {}
                    }
                }
            } catch (e: Exception) {
                Log.w(TAG, "Failed to silence active playback", e)
            }
        }

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
    private var mixerAttributesPending: Boolean = false
    private var sampleRate: Int = 44100
    private var bitDepth: Int = 16
    private var channelCount: Int = 2
    private var chosenEncoding: Int = AudioFormat.ENCODING_PCM_16BIT
    private var floatWriteBuffer = FloatArray(16384)
    private var bytesPerFrame: Int = 4
    private var framesWritten: Long = 0
    private var eosSignaled: Boolean = false
    private var eosTimestamp: Long = 0L
    private var lastHeadPosition: Long = 0L
    private var lastHeadAdvanceTimestamp: Long = 0L
    private var drainFinished: Boolean = false
    private var lastReportedPositionMs: Long = 0L
    private var lastUnderrunCount: Int = 0
    private var appContext: Context? = null
    private var wakeLock: PowerManager.WakeLock? = null
    private var wifiLock: WifiManager.WifiLock? = null
    private var currentTitle: String = "Aria"
    private var currentArtist: String = "Playing audio"

    fun getAudioSpec(): String {
        val khz = sampleRate / 1000.0
        val khzStr = if (khz % 1.0 == 0.0) String.format("%.0f", khz) else String.format("%.1f", khz)
        return "$bitDepth-bit / $khzStr kHz"
    }

    fun setTrackMetadata(title: String?, artist: String?) {
        if (!title.isNullOrBlank()) this.currentTitle = title
        if (!artist.isNullOrBlank()) this.currentArtist = artist
        appContext?.let {
            val isPlaying = audioTrack?.playState == AudioTrack.PLAYSTATE_PLAYING
            AriaPlaybackService.updatePlayback(it, this.currentTitle, this.currentArtist, getAudioSpec(), isPlaying)
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
                val wifiMode = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    WifiManager.WIFI_MODE_FULL_LOW_LATENCY
                } else {
                    @Suppress("DEPRECATION")
                    WifiManager.WIFI_MODE_FULL_HIGH_PERF
                }
                wifiLock = wm?.createWifiLock(wifiMode, "Aria:PlaybackWifiLock")?.apply {
                    setReferenceCounted(false)
                }
            }
            wifiLock?.let {
                if (!it.isHeld) {
                    it.acquire()
                    AriaPlaybackService.logDiagnostic(2, "Playback", "Acquired AudioTrack WifiLock")
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

    private fun applyMixerAttributes() {
        if (Build.VERSION.SDK_INT < 34) return
        val am = audioManager ?: return
        val dev = targetDevice ?: return
        val attrs = audioAttributes ?: return
        if (AriaBluetoothAudioHelper.isBluetoothDevice(dev)) return

        try {
            val channelMask = if (channelCount == 1) {
                AudioFormat.CHANNEL_OUT_MONO
            } else {
                AudioFormat.CHANNEL_OUT_STEREO
            }
            val audioFormat = AudioFormat.Builder()
                .setSampleRate(sampleRate)
                .setChannelMask(channelMask)
                .setEncoding(chosenEncoding)
                .build()

            val supportedMixers = am.getSupportedMixerAttributes(dev)
            Log.i(TAG, "Device ${dev.productName} supported mixer configurations: ${supportedMixers.size}")

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
                attrs,
                dev,
                bitPerfectAttr
            )
            if (setOk) {
                mixerAttributesConfigured = true
                mixerAttributesPending = false
                Log.i(TAG, "Successfully enabled built-in MIXER_BEHAVIOR_BIT_PERFECT on ${dev.productName} at native rate ${sampleRate}Hz")
            } else {
                Log.w(TAG, "setPreferredMixerAttributes returned false for ${dev.productName}")
            }
        } catch (e: Exception) {
            Log.w(TAG, "Failed to set preferred mixer attributes: ${e.message}")
        }
    }

    @JvmOverloads
    fun open(
        context: Context,
        deviceId: Int,
        sampleRate: Int,
        channelCount: Int,
        bitDepth: Int,
        isPrewarm: Boolean = false
    ): Int {
        try {
            this.appContext = context.applicationContext
            val am = context.getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: run {
                Log.e(TAG, "AudioManager not available")
                return FORMAT_FAILED
            }
            this.audioManager = am
            this.sampleRate = sampleRate
            this.bitDepth = if (bitDepth > 0) bitDepth else 16
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
            var effectiveSampleRate = sampleRate
            if (isBluetooth) {
                Log.i(TAG, "Bluetooth output device targeted: ${device.productName} (id=${device.id}). Configuring for native source rate ${sampleRate}Hz (${bitDepth}-bit)...")
                val btInfo = AriaBluetoothAudioHelper.optimizeForHighestQuality(context, sampleRate, bitDepth)
                if (btInfo.sampleRate > 0) {
                    effectiveSampleRate = btInfo.sampleRate
                }
                Log.i(TAG, "Bluetooth HD audio active: ${btInfo.summary}, effectiveSampleRate=${effectiveSampleRate}Hz")
            } else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M && device.sampleRates.isNotEmpty()) {
                val supported = device.sampleRates
                if (!supported.contains(sampleRate)) {
                    effectiveSampleRate = selectNearestSupportedRate(supported, sampleRate)
                    Log.i(TAG, "Device does not support ${sampleRate}Hz; selected nearest supported rate ${effectiveSampleRate}Hz")
                }
            }

            val sampleRate = effectiveSampleRate
            this.sampleRate = sampleRate
            this.bitDepth = if (bitDepth > 0) bitDepth else 16
            this.channelCount = channelCount

            val channelMask = if (channelCount == 1) {
                AudioFormat.CHANNEL_OUT_MONO
            } else {
                AudioFormat.CHANNEL_OUT_STEREO
            }

            var chosenEncoding = when {
                bitDepth <= 16 -> AudioFormat.ENCODING_PCM_16BIT
                isBluetooth -> {
                    // For Bluetooth devices, AudioFlinger processes ENCODING_PCM_FLOAT with 32-bit float
                    // fidelity to feed the Bluetooth audio encoder cleanly without byte packing driver noise.
                    AudioFormat.ENCODING_PCM_FLOAT
                }
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
            this.chosenEncoding = chosenEncoding

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
                val currentActive = activeInstance
                if (isPrewarm && currentActive != null && currentActive.sampleRate != sampleRate) {
                    mixerAttributesPending = true
                    Log.i(TAG, "Deferring USB DAC preferred mixer attribute switch to ${sampleRate}Hz until handover")
                } else {
                    applyMixerAttributes()
                }
            }

            // 2. Build AudioTrack matching the exact native stream parameters
            // Buffer at least 750ms of audio (or 8x minBufferSize) to protect against background GC pauses and CPU frequency scaling
            val bufferDurationMs = 750
            val targetBufferSize = maxOf(minBufferSize * 8, (sampleRate * bytesPerFrame * bufferDurationMs) / 1000)
            val bufferCapacityMs = if (sampleRate > 0 && bytesPerFrame > 0) (targetBufferSize * 1000L) / (sampleRate * bytesPerFrame) else 0L

            AriaPlaybackService.logDiagnostic(
                2, // INFO
                "Playback",
                "Building AudioTrack: bufferSize=$targetBufferSize bytes (~${bufferCapacityMs}ms cushion, minBufferSize=$minBufferSize), encoding=$chosenEncoding, isBluetooth=$isBluetooth"
            )

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
            this.eosSignaled = false
            this.eosTimestamp = 0L
            this.lastHeadPosition = 0L
            this.lastHeadAdvanceTimestamp = 0L
            this.drainFinished = false
            this.lastReportedPositionMs = 0L
            if (!isPrewarm) {
                activeInstance = this
            }

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
                track.addOnRoutingChangedListener(AudioTrack.OnRoutingChangedListener { router ->
                    val routed = (router as? AudioTrack)?.routedDevice
                    if (routed != null) {
                        val isSpeaker = routed.type == AudioDeviceInfo.TYPE_BUILTIN_SPEAKER ||
                                        routed.type == AudioDeviceInfo.TYPE_BUILTIN_EARPIECE
                        val context = appContext
                        if (isSpeaker && context != null && !AriaPlaybackService.hasExternalAudioOutput(context)) {
                            Log.i(TAG, "AudioTrack routed to built-in speaker with no external devices -> stopping playback immediately")
                            try {
                                router.pause()
                                router.flush()
                            } catch (_: Exception) {}
                            AriaPlaybackService.onAudioReroutedToSpeaker()
                        }
                    }
                }, null)
            }

            Log.i(TAG, "AudioTrack initialized successfully in bit-accurate mode (encoding=$chosenEncoding, buffer size=$targetBufferSize bytes)")
            return formatCode
        } catch (e: Exception) {
            Log.e(TAG, "Error opening bit-accurate audio track", e)
            return FORMAT_FAILED
        }
    }

    fun getSampleRate(): Int = this.sampleRate

    private fun selectNearestSupportedRate(supported: IntArray, sourceRate: Int): Int {
        if (supported.contains(sourceRate)) return sourceRate
        val higher = supported.filter { it > sourceRate }
        if (higher.isNotEmpty()) {
            return higher.minByOrNull { it - sourceRate }!!
        }
        val lower = supported.filter { it < sourceRate }
        if (lower.isNotEmpty()) {
            return lower.minByOrNull { sourceRate - it }!!
        }
        return supported.minByOrNull { Math.abs(it - sourceRate) } ?: sourceRate
    }

    private var writeBuffer = ByteArray(65536)

    fun getWriteBuffer(capacity: Int): ByteArray {
        if (writeBuffer.size < capacity) {
            writeBuffer = ByteArray(capacity)
        }
        return writeBuffer
    }

    fun writeDirect(size: Int): Int {
        val track = audioTrack ?: return -1
        return try {
            val written = if (chosenEncoding == AudioFormat.ENCODING_PCM_FLOAT) {
                val numFloats = size / 4
                if (floatWriteBuffer.size < numFloats) {
                    floatWriteBuffer = FloatArray(numFloats)
                }
                ByteBuffer.wrap(writeBuffer, 0, size)
                    .order(ByteOrder.LITTLE_ENDIAN)
                    .asFloatBuffer()
                    .get(floatWriteBuffer, 0, numFloats)
                val floatsWritten = track.write(floatWriteBuffer, 0, numFloats, AudioTrack.WRITE_BLOCKING)
                if (floatsWritten > 0) floatsWritten * 4 else floatsWritten
            } else {
                track.write(writeBuffer, 0, size, AudioTrack.WRITE_BLOCKING)
            }

            if (written > 0 && bytesPerFrame > 0) {
                framesWritten += written / bytesPerFrame
            } else if (written < 0) {
                AriaPlaybackService.logDiagnostic(
                    4, // ERROR
                    "Playback",
                    "AudioTrack.write failed with code $written (encoding=$chosenEncoding, size=$size)"
                )
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
                val underruns = track.underrunCount
                if (underruns > lastUnderrunCount) {
                    val diff = underruns - lastUnderrunCount
                    lastUnderrunCount = underruns
                    AriaPlaybackService.logDiagnostic(
                        3, // WARN
                        "Playback",
                        "AudioTrack hardware underrun detected! +$diff underrun(s) (total=$underruns, framesWritten=$framesWritten)"
                    )
                }
            }
            written
        } catch (e: Exception) {
            Log.e(TAG, "Error writing audio data directly", e)
            -1
        }
    }

    fun write(data: ByteArray, offset: Int, size: Int): Int {
        val track = audioTrack ?: return -1
        return try {
            val written = if (chosenEncoding == AudioFormat.ENCODING_PCM_FLOAT) {
                val numFloats = size / 4
                if (floatWriteBuffer.size < numFloats) {
                    floatWriteBuffer = FloatArray(numFloats)
                }
                ByteBuffer.wrap(data, offset, size)
                    .order(ByteOrder.LITTLE_ENDIAN)
                    .asFloatBuffer()
                    .get(floatWriteBuffer, 0, numFloats)
                val floatsWritten = track.write(floatWriteBuffer, 0, numFloats, AudioTrack.WRITE_BLOCKING)
                if (floatsWritten > 0) floatsWritten * 4 else floatsWritten
            } else {
                track.write(data, offset, size, AudioTrack.WRITE_BLOCKING)
            }
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
            activeInstance = this
            acquireLocks()
            appContext?.let {
                val t = if (currentTitle != "Aria") currentTitle else null
                val a = if (currentArtist != "Playing audio") currentArtist else null
                AriaPlaybackService.startPlayback(it, t, a, getAudioSpec())
            }
            if (Build.VERSION.SDK_INT >= 34 && mixerAttributesPending && !mixerAttributesConfigured) {
                applyMixerAttributes()
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
                val t = if (currentTitle != "Aria") currentTitle else null
                val a = if (currentArtist != "Playing audio") currentArtist else null
                AriaPlaybackService.updatePlayback(it, t, a, getAudioSpec(), false)
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
            eosSignaled = true
            eosTimestamp = System.currentTimeMillis()
            lastHeadPosition = (track.playbackHeadPosition.toLong() and 0xFFFFFFFFL)
            lastHeadAdvanceTimestamp = eosTimestamp
            drainFinished = false
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
        if (drainFinished) return true
        if (framesWritten <= 0) return true

        return try {
            val head = track.playbackHeadPosition.toLong() and 0xFFFFFFFFL
            val now = System.currentTimeMillis()

            // 1. Reached or exceeded frames written
            if (head >= framesWritten) {
                drainFinished = true
                return true
            }

            // 2. Tolerance check: within 100ms of framesWritten
            val toleranceFrames = if (sampleRate > 0) sampleRate / 10 else 4410
            if (head >= framesWritten - toleranceFrames) {
                drainFinished = true
                return true
            }

            // 3. If EOS was signaled, check for position reset, stall, or safety timeout
            if (eosSignaled) {
                // If head reset to 0 (or dropped significantly) after having played audio, HAL finished playback!
                if (head < lastHeadPosition && lastHeadPosition > toleranceFrames) {
                    drainFinished = true
                    return true
                }

                // If head has not advanced for 150ms after EOS, drain is complete
                if (head > lastHeadPosition) {
                    lastHeadPosition = head
                    lastHeadAdvanceTimestamp = now
                } else if (now - lastHeadAdvanceTimestamp > 150L) {
                    drainFinished = true
                    return true
                }

                // Safety timeout: max buffer duration is ~3 seconds, so drain cannot take > 3.5s
                if (now - eosTimestamp > 3500L) {
                    drainFinished = true
                    return true
                }
            }

            false
        } catch (e: Exception) {
            true
        }
    }

    fun getPositionMs(): Long {
        val track = audioTrack ?: return 0L
        return try {
            if (drainFinished && sampleRate > 0 && framesWritten > 0) {
                return (framesWritten * 1000L) / sampleRate
            }
            val head = track.playbackHeadPosition.toLong() and 0xFFFFFFFFL
            if (sampleRate > 0) {
                val pos = (head * 1000L) / sampleRate
                if (eosSignaled && pos < lastReportedPositionMs) {
                    lastReportedPositionMs
                } else {
                    lastReportedPositionMs = pos
                    pos
                }
            } else {
                0L
            }
        } catch (e: Exception) {
            0L
        }
    }

    fun close(): Boolean {
        if (activeInstance === this) {
            activeInstance = null
        }
        releaseLocks()
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
                if (activeInstance === this || activeInstance == null) {
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
