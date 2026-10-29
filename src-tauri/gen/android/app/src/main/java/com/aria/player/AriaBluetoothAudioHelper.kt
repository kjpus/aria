package com.aria.player

import android.bluetooth.BluetoothA2dp
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.content.Context
import android.media.AudioDeviceInfo
import android.media.AudioFormat
import android.media.AudioManager
import android.os.Build
import android.util.Log

data class BluetoothAudioInfo(
    val isConnected: Boolean,
    val deviceName: String = "",
    val deviceId: Int = 0,
    val codecType: Int = -1,
    val codecName: String = "Unknown",
    val sampleRate: Int = 0,
    val bitDepth: Int = 0,
    val bitrateKbps: Int = 0,
    val bitrateDescription: String = "",
    val hdAudioSupported: Boolean = false,
    val hdAudioEnabled: Boolean = false,
    val isHighResolution: Boolean = false,
    val summary: String = ""
)

object AriaBluetoothAudioHelper {
    private const val TAG = "AriaBluetoothAudio"

    // Bluetooth A2DP Codec Type constants (matching AOSP BluetoothCodecConfig)
    const val CODEC_TYPE_SBC = 0
    const val CODEC_TYPE_AAC = 1
    const val CODEC_TYPE_APTX = 2
    const val CODEC_TYPE_APTX_HD = 3
    const val CODEC_TYPE_LDAC = 4
    const val CODEC_TYPE_LC3 = 5
    const val CODEC_TYPE_OPUS = 6

    // Sample rate bitmasks
    const val SAMPLE_RATE_NONE = 0
    const val SAMPLE_RATE_44100 = 0x1
    const val SAMPLE_RATE_48000 = 0x2
    const val SAMPLE_RATE_88200 = 0x4
    const val SAMPLE_RATE_96000 = 0x8
    const val SAMPLE_RATE_176400 = 0x10
    const val SAMPLE_RATE_192000 = 0x20

    // Bits per sample bitmasks
    const val BITS_PER_SAMPLE_NONE = 0
    const val BITS_PER_SAMPLE_16 = 0x1
    const val BITS_PER_SAMPLE_24 = 0x2
    const val BITS_PER_SAMPLE_32 = 0x4

    // LDAC Quality Modes (in codecSpecific1)
    const val LDAC_QUALITY_HIGH = 1000L      // 990 kbps (High Quality / Sound Quality Preferred)
    const val LDAC_QUALITY_MID = 1001L       // 660 kbps (Standard)
    const val LDAC_QUALITY_LOW = 1002L       // 330 kbps (Connection Preferred)
    const val LDAC_QUALITY_ADAPTIVE = 1003L  // Best effort (adaptive bitrate)

    @Volatile
    private var bluetoothA2dp: BluetoothA2dp? = null
    @Volatile
    private var isProxyBinding = false
    private var cachedAppContext: Context? = null

    private val profileListener = object : BluetoothProfile.ServiceListener {
        override fun onServiceConnected(profile: Int, proxy: BluetoothProfile) {
            if (profile == BluetoothProfile.A2DP) {
                bluetoothA2dp = proxy as? BluetoothA2dp
                isProxyBinding = false
                Log.i(TAG, "Bluetooth A2DP profile proxy connected")
            }
        }

        override fun onServiceDisconnected(profile: Int) {
            if (profile == BluetoothProfile.A2DP) {
                bluetoothA2dp = null
                isProxyBinding = false
                Log.i(TAG, "Bluetooth A2DP profile proxy disconnected")
            }
        }
    }

    @JvmStatic
    fun initialize(context: Context) {
        val appCtx = context.applicationContext ?: context
        cachedAppContext = appCtx
        ensureA2dpProxy(appCtx)
    }

    @JvmStatic
    fun cleanup() {
        try {
            val a2dp = bluetoothA2dp
            val adapter = getBluetoothAdapter(cachedAppContext)
            if (adapter != null && a2dp != null) {
                adapter.closeProfileProxy(BluetoothProfile.A2DP, a2dp)
            }
        } catch (e: Exception) {
            Log.w(TAG, "Error closing A2DP profile proxy: ${e.message}")
        } finally {
            bluetoothA2dp = null
            isProxyBinding = false
        }
    }

    fun ensureA2dpProxy(context: Context) {
        if (bluetoothA2dp != null || isProxyBinding) return
        val adapter = getBluetoothAdapter(context) ?: return
        if (!adapter.isEnabled) return

        try {
            isProxyBinding = true
            val ok = adapter.getProfileProxy(context.applicationContext ?: context, profileListener, BluetoothProfile.A2DP)
            if (!ok) {
                isProxyBinding = false
                Log.w(TAG, "getProfileProxy returned false for BluetoothProfile.A2DP")
            }
        } catch (e: Exception) {
            isProxyBinding = false
            Log.w(TAG, "Failed to getProfileProxy for A2DP: ${e.message}")
        }
    }

    private fun getBluetoothAdapter(context: Context?): BluetoothAdapter? {
        if (context != null && Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            val bm = context.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager
            if (bm?.adapter != null) return bm.adapter
        }
        @Suppress("DEPRECATION")
        return BluetoothAdapter.getDefaultAdapter()
    }

    @JvmStatic
    fun isBluetoothDevice(deviceInfo: AudioDeviceInfo): Boolean {
        if (deviceInfo.type == AudioDeviceInfo.TYPE_BLUETOOTH_A2DP) return true
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            if (deviceInfo.type == AudioDeviceInfo.TYPE_BLE_HEADSET ||
                deviceInfo.type == AudioDeviceInfo.TYPE_BLE_SPEAKER ||
                deviceInfo.type == AudioDeviceInfo.TYPE_BLE_BROADCAST) {
                return true
            }
        }
        return false
    }

    @JvmStatic
    fun isBluetoothDeviceId(context: Context?, deviceId: Int): Boolean {
        val ctx = context ?: cachedAppContext ?: return false
        val am = ctx.getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: return false
        val devices = am.getDevices(AudioManager.GET_DEVICES_OUTPUTS)
        val dev = devices.find { it.id == deviceId } ?: return false
        return isBluetoothDevice(dev)
    }

    @JvmStatic
    fun hasBluetoothOutput(context: Context?): Boolean {
        val ctx = context ?: cachedAppContext ?: return false
        val am = ctx.getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: return false
        val devices = am.getDevices(AudioManager.GET_DEVICES_OUTPUTS)
        return devices.any { isBluetoothDevice(it) }
    }

    /**
     * Inspects the active Bluetooth device and current audio routing to determine
     * the active codec, sample rate, bit depth, and bitrate.
     */
    @JvmStatic
    fun getBluetoothAudioInfo(context: Context?): BluetoothAudioInfo {
        val ctx = context ?: cachedAppContext ?: return BluetoothAudioInfo(isConnected = false)
        ensureA2dpProxy(ctx)

        val am = ctx.getSystemService(Context.AUDIO_SERVICE) as? AudioManager
        val audioDevices = am?.getDevices(AudioManager.GET_DEVICES_OUTPUTS) ?: emptyArray()
        val btAudioDevice = audioDevices.find { isBluetoothDevice(it) }

        val a2dp = bluetoothA2dp
        val btDevice = getActiveBluetoothDevice(a2dp, btAudioDevice)

        if (btAudioDevice == null && btDevice == null) {
            return BluetoothAudioInfo(isConnected = false)
        }

        val deviceName = btDevice?.name ?: btAudioDevice?.productName?.toString() ?: "Bluetooth Audio"
        val deviceId = btAudioDevice?.id ?: 0

        var hdSupported = false
        var hdEnabled = false
        if (a2dp != null && btDevice != null) {
            hdSupported = checkOptionalCodecsSupported(a2dp, btDevice)
            hdEnabled = checkOptionalCodecsEnabled(a2dp, btDevice)
        }

        // Query active codec configuration from BluetoothA2dp / BluetoothCodecStatus
        val codecConfig = if (a2dp != null && btDevice != null) {
            getActiveCodecConfig(a2dp, btDevice)
        } else {
            null
        }

        if (codecConfig != null) {
            val codecType = getCodecType(codecConfig)
            val sampleRate = sampleRateMaskToHz(getSampleRateMask(codecConfig))
            val bitDepth = bitsPerSampleMaskToBits(getBitsPerSampleMask(codecConfig))
            val codecSpecific1 = getCodecSpecific1(codecConfig)
            val codecName = formatCodecName(codecType, codecConfig)
            val bitrateKbps = formatBitrateKbps(codecType, codecSpecific1, sampleRate, codecConfig)
            val bitrateDesc = formatBitrateDescription(codecType, codecSpecific1, bitrateKbps, codecConfig)
            val isHighRes = (codecType == CODEC_TYPE_LDAC || codecType == CODEC_TYPE_APTX_HD ||
                codecName.contains("SSC") || codecName.contains("Seamless") || codecName.contains("Scalable") ||
                sampleRate > 48000 || bitDepth > 16)
            val summary = "$codecName ${sampleRate / 1000.0}kHz / $bitDepth-bit @ $bitrateDesc"

            return BluetoothAudioInfo(
                isConnected = true,
                deviceName = deviceName,
                deviceId = deviceId,
                codecType = codecType,
                codecName = codecName,
                sampleRate = sampleRate,
                bitDepth = bitDepth,
                bitrateKbps = bitrateKbps,
                bitrateDescription = bitrateDesc,
                hdAudioSupported = hdSupported,
                hdAudioEnabled = hdEnabled,
                isHighResolution = isHighRes,
                summary = summary
            )
        }

        // Fallback: estimate from AudioDeviceInfo
        val encodings = btAudioDevice?.encodings ?: intArrayOf()
        val sampleRates = btAudioDevice?.sampleRates ?: intArrayOf()
        val estimatedBitDepth = if (encodings.any { it == AudioFormat.ENCODING_PCM_24BIT_PACKED || it == AudioFormat.ENCODING_PCM_32BIT || it == AudioFormat.ENCODING_PCM_FLOAT }) 24 else 16
        val estimatedSampleRate = sampleRates.maxOrNull() ?: 44100
        val summary = "Bluetooth Audio ${estimatedSampleRate / 1000.0}kHz / $estimatedBitDepth-bit"

        return BluetoothAudioInfo(
            isConnected = true,
            deviceName = deviceName,
            deviceId = deviceId,
            codecName = if (hdSupported) "HD Audio" else "Standard Bluetooth",
            sampleRate = estimatedSampleRate,
            bitDepth = estimatedBitDepth,
            hdAudioSupported = hdSupported,
            hdAudioEnabled = hdEnabled,
            summary = summary
        )
    }

    /**
     * Figures out the highest quality codec, bitrate, sample rate, and bit depth
     * supported by BOTH the phone and connected earbuds/headset, enables HD audio,
     * and sets the codec configuration preference to the maximum tier (e.g. LDAC 990 kbps).
     */
    @JvmOverloads
    @JvmStatic
    fun optimizeForHighestQuality(context: Context?, sourceSampleRate: Int = 44100): BluetoothAudioInfo {
        val ctx = context ?: cachedAppContext ?: return BluetoothAudioInfo(isConnected = false)
        ensureA2dpProxy(ctx)

        val am = ctx.getSystemService(Context.AUDIO_SERVICE) as? AudioManager
        val audioDevices = am?.getDevices(AudioManager.GET_DEVICES_OUTPUTS) ?: emptyArray()
        val btAudioDevice = audioDevices.find { isBluetoothDevice(it) }

        val a2dp = bluetoothA2dp
        val btDevice = getActiveBluetoothDevice(a2dp, btAudioDevice)

        if (btDevice == null || a2dp == null) {
            Log.w(TAG, "No active Bluetooth A2DP device or proxy not connected yet; returning current audio info")
            return getBluetoothAudioInfo(ctx)
        }

        Log.i(TAG, "Optimizing Bluetooth audio quality for device: ${btDevice.name} at source rate: ${sourceSampleRate}Hz (NEVER UPSAMPLING)")

        // 1. Enable HD Audio (optional codecs) on the device if supported
        val hdSupported = checkOptionalCodecsSupported(a2dp, btDevice)
        val hdEnabled = checkOptionalCodecsEnabled(a2dp, btDevice)
        if (hdSupported && !hdEnabled) {
            try {
                val setEnabledMethod = a2dp.javaClass.getMethod(
                    "setOptionalCodecsEnabled",
                    BluetoothDevice::class.java,
                    Int::class.javaPrimitiveType
                )
                setEnabledMethod.invoke(a2dp, btDevice, 1) // OPTIONAL_CODECS_PREF_ENABLED = 1
                Log.i(TAG, "Enabled HD audio / optional codecs for ${btDevice.name}")
            } catch (e: Exception) {
                Log.d(TAG, "setOptionalCodecsEnabled reflection failed: ${e.message}")
            }
        }

        // 2. Query selectable capabilities (the intersection of phone SoC and headset capabilities)
        val selectableCaps = getSelectableCapabilities(a2dp, btDevice)
        if (selectableCaps.isNotEmpty()) {
            Log.i(TAG, "Discovered ${selectableCaps.size} selectable Bluetooth codec capabilities from phone & headset")

            // Rank codecs to select highest quality codec family:
            // LDAC / Samsung Seamless Codec UHQ > aptX HD / SSC Hi-Fi > LC3 > aptX > AAC > SBC
            val bestConfig = selectableCaps.maxByOrNull { config ->
                val type = getCodecType(config)
                val rank = getCodecRank(type, config)
                val srMask = getSampleRateMask(config)
                val bpsMask = getBitsPerSampleMask(config)
                val maxSr = highestSampleRateFromMask(srMask)
                val maxBps = highestBitsPerSampleFromMask(bpsMask)
                (rank * 1000000) + (maxSr * 10) + maxBps
            }

            if (bestConfig != null) {
                val bestType = getCodecType(bestConfig)
                val bestSrMask = getSampleRateMask(bestConfig)
                val bestBpsMask = getBitsPerSampleMask(bestConfig)

                // CRITICAL AUDIOPHILE RULE: NEVER UP-SAMPLE.
                // If source track is 44.1kHz, configure Bluetooth codec to 44.1kHz,
                // even if earbuds or phone support 96kHz or 192kHz!
                val targetSampleRateFlag = selectSampleRateFlagNeverUpsample(bestSrMask, sourceSampleRate)
                val targetBitsPerSampleFlag = selectHighestBitsPerSampleFlag(bestBpsMask)

                // For LDAC, force 1000L (Sound Quality Preferred / 990 kbps for 48k/96k, 909 kbps for 44.1k/88.2k)
                val targetCodecSpecific1 = if (bestType == CODEC_TYPE_LDAC) LDAC_QUALITY_HIGH else 0L

                val negotiatedHz = sampleRateMaskToHz(targetSampleRateFlag)
                Log.i(TAG, "Never-upsample policy: source=${sourceSampleRate}Hz -> Bluetooth codec=${formatCodecName(bestType, bestConfig)} @ ${negotiatedHz}Hz (flag=0x${Integer.toHexString(targetSampleRateFlag)}), " +
                    "bitsPerSampleMask=0x${Integer.toHexString(targetBitsPerSampleFlag)}, codecSpecific1=$targetCodecSpecific1")

                tryApplyCodecPreference(a2dp, btDevice, bestType, targetSampleRateFlag, targetBitsPerSampleFlag, targetCodecSpecific1)
            }
        } else {
            Log.i(TAG, "No explicit selectable codec capabilities returned; ensuring HD audio preference is applied")
        }

        return getBluetoothAudioInfo(ctx)
    }

    private fun tryApplyCodecPreference(
        a2dp: BluetoothA2dp,
        device: BluetoothDevice,
        codecType: Int,
        sampleRateFlag: Int,
        bitsPerSampleFlag: Int,
        codecSpecific1: Long
    ) {
        // Try Approach 1: BluetoothCodecConfig.Builder (Android 11+)
        try {
            val builderClass = Class.forName("android.bluetooth.BluetoothCodecConfig\$Builder")
            val builder = builderClass.getDeclaredConstructor().newInstance()

            builderClass.getMethod("setCodecType", Int::class.javaPrimitiveType).invoke(builder, codecType)
            builderClass.getMethod("setCodecPriority", Int::class.javaPrimitiveType).invoke(builder, 1000000) // CODEC_PRIORITY_HIGHEST
            builderClass.getMethod("setSampleRate", Int::class.javaPrimitiveType).invoke(builder, sampleRateFlag)
            builderClass.getMethod("setBitsPerSample", Int::class.javaPrimitiveType).invoke(builder, bitsPerSampleFlag)
            builderClass.getMethod("setChannelMode", Int::class.javaPrimitiveType).invoke(builder, 2) // STEREO = 0x2
            if (codecSpecific1 != 0L) {
                builderClass.getMethod("setCodecSpecific1", Long::class.javaPrimitiveType).invoke(builder, codecSpecific1)
            }

            val newConfig = builderClass.getMethod("build").invoke(builder)
            val configClass = Class.forName("android.bluetooth.BluetoothCodecConfig")
            val setMethod = a2dp.javaClass.getMethod("setCodecConfigPreference", BluetoothDevice::class.java, configClass)
            setMethod.invoke(a2dp, device, newConfig)
            Log.i(TAG, "Successfully requested preferred codec configuration via Builder")
            return
        } catch (e: Exception) {
            Log.d(TAG, "Builder method for setCodecConfigPreference not available: ${e.message}")
        }

        // Try Approach 2: Direct constructor reflection (Android 8.0 - 10)
        try {
            val configClass = Class.forName("android.bluetooth.BluetoothCodecConfig")
            val constructor = configClass.getDeclaredConstructor(
                Int::class.javaPrimitiveType,
                Int::class.javaPrimitiveType,
                Int::class.javaPrimitiveType,
                Int::class.javaPrimitiveType,
                Int::class.javaPrimitiveType,
                Long::class.javaPrimitiveType,
                Long::class.javaPrimitiveType,
                Long::class.javaPrimitiveType,
                Long::class.javaPrimitiveType
            )
            val newConfig = constructor.newInstance(
                codecType,
                1000000, // CODEC_PRIORITY_HIGHEST
                sampleRateFlag,
                bitsPerSampleFlag,
                2, // STEREO
                codecSpecific1,
                0L,
                0L,
                0L
            )
            val setMethod = a2dp.javaClass.getMethod("setCodecConfigPreference", BluetoothDevice::class.java, configClass)
            setMethod.invoke(a2dp, device, newConfig)
            Log.i(TAG, "Successfully requested preferred codec configuration via constructor")
        } catch (e: Exception) {
            Log.d(TAG, "setCodecConfigPreference system restricted: ${e.message}")
        }
    }

    private fun getActiveBluetoothDevice(a2dp: BluetoothA2dp?, audioDevice: AudioDeviceInfo?): BluetoothDevice? {
        if (a2dp != null) {
            try {
                val getActiveMethod = a2dp.javaClass.getMethod("getActiveDevice")
                val dev = getActiveMethod.invoke(a2dp) as? BluetoothDevice
                if (dev != null) return dev
            } catch (_: Exception) {}

            try {
                val connected = a2dp.connectedDevices
                if (!connected.isNullOrEmpty()) {
                    if (audioDevice != null) {
                        val match = connected.find { it.name.equals(audioDevice.productName?.toString(), ignoreCase = true) }
                        if (match != null) return match
                    }
                    return connected.first()
                }
            } catch (_: Exception) {}
        }
        return null
    }

    private fun checkOptionalCodecsSupported(a2dp: BluetoothA2dp, device: BluetoothDevice): Boolean {
        return try {
            val method = a2dp.javaClass.getMethod("isOptionalCodecsSupported", BluetoothDevice::class.java)
            val res = method.invoke(a2dp, device) as? Int ?: 0
            res == 1 // OPTIONAL_CODECS_SUPPORTED = 1
        } catch (_: Exception) {
            false
        }
    }

    private fun checkOptionalCodecsEnabled(a2dp: BluetoothA2dp, device: BluetoothDevice): Boolean {
        return try {
            val method = a2dp.javaClass.getMethod("isOptionalCodecsEnabled", BluetoothDevice::class.java)
            val res = method.invoke(a2dp, device) as? Int ?: 0
            res == 1 // OPTIONAL_CODECS_PREF_ENABLED = 1
        } catch (_: Exception) {
            false
        }
    }

    private fun getActiveCodecConfig(a2dp: BluetoothA2dp, device: BluetoothDevice): Any? {
        return try {
            val getCodecStatusMethod = a2dp.javaClass.getMethod("getCodecStatus", BluetoothDevice::class.java)
            val status = getCodecStatusMethod.invoke(a2dp, device) ?: return null
            val getCodecConfigMethod = status.javaClass.getMethod("getCodecConfig")
            getCodecConfigMethod.invoke(status)
        } catch (e: Exception) {
            Log.d(TAG, "getActiveCodecConfig error: ${e.message}")
            null
        }
    }

    private fun getSelectableCapabilities(a2dp: BluetoothA2dp, device: BluetoothDevice): List<Any> {
        return try {
            val getCodecStatusMethod = a2dp.javaClass.getMethod("getCodecStatus", BluetoothDevice::class.java)
            val status = getCodecStatusMethod.invoke(a2dp, device) ?: return emptyList()
            val getSelectableMethod = status.javaClass.getMethod("getCodecsSelectableCapabilities")
            val res = getSelectableMethod.invoke(status)
            when (res) {
                is Array<*> -> res.filterNotNull()
                is List<*> -> res.filterNotNull()
                else -> emptyList()
            }
        } catch (e: Exception) {
            Log.d(TAG, "getSelectableCapabilities error: ${e.message}")
            emptyList()
        }
    }

    private fun getCodecType(config: Any): Int {
        return try {
            val method = config.javaClass.getMethod("getCodecType")
            method.invoke(config) as? Int ?: -1
        } catch (_: Exception) {
            -1
        }
    }

    private fun getSampleRateMask(config: Any): Int {
        return try {
            val method = config.javaClass.getMethod("getSampleRate")
            method.invoke(config) as? Int ?: 0
        } catch (_: Exception) {
            0
        }
    }

    private fun getBitsPerSampleMask(config: Any): Int {
        return try {
            val method = config.javaClass.getMethod("getBitsPerSample")
            method.invoke(config) as? Int ?: 0
        } catch (_: Exception) {
            0
        }
    }

    private fun getCodecSpecific1(config: Any): Long {
        return try {
            val method = config.javaClass.getMethod("getCodecSpecific1")
            method.invoke(config) as? Long ?: 0L
        } catch (_: Exception) {
            0L
        }
    }

    private fun getCodecRank(codecType: Int, config: Any?): Int {
        var isSamsungSSC = false
        if (config != null) {
            try {
                val nameMethod = config.javaClass.getMethod("getCodecName")
                val name = nameMethod.invoke(config) as? String
                if (name != null && (name.contains("Seamless", ignoreCase = true) ||
                        name.contains("Scalable", ignoreCase = true) ||
                        name.contains("SSC", ignoreCase = true) ||
                        name.contains("Samsung", ignoreCase = true))) {
                    isSamsungSSC = true
                }
            } catch (_: Exception) {}

            if (!isSamsungSSC) {
                val str = config.toString()
                if (str.contains("Seamless", ignoreCase = true) ||
                    str.contains("Scalable", ignoreCase = true) ||
                    str.contains("SSC", ignoreCase = true)) {
                    isSamsungSSC = true
                }
            }
        }

        if (isSamsungSSC) {
            val sr = if (config != null) sampleRateMaskToHz(getSampleRateMask(config)) else 0
            val bps = if (config != null) bitsPerSampleMaskToBits(getBitsPerSampleMask(config)) else 0
            return if (sr >= 96000) {
                98 // Samsung Seamless Codec (SSC UHQ - 24-bit 96kHz): top-tier flagships (S24/S25/S26 + Buds3 Pro)
            } else if (bps > 16 || sr >= 48000) {
                82 // Samsung Seamless Codec (SSC Hi-Fi - 24-bit 48kHz): Buds2 Pro / One UI 4+
            } else {
                55 // Scalable 16-bit baseline
            }
        }

        return when (codecType) {
            CODEC_TYPE_LDAC -> 100        // Sony LDAC: 990 kbps, 96kHz, 24/32-bit (Highest)
            CODEC_TYPE_APTX_HD -> 80      // Qualcomm aptX HD: 576 kbps, 48kHz, 24-bit
            CODEC_TYPE_LC3 -> 75          // LE Audio LC3
            CODEC_TYPE_APTX -> 60         // aptX: 384 kbps, 48kHz, 16-bit
            CODEC_TYPE_AAC -> 50          // AAC: ~256-320 kbps
            CODEC_TYPE_SBC -> 10          // SBC: baseline standard ~328 kbps
            else -> if (codecType > 4) 85 else 20 // other vendor extension codecs (LHDC, aptX Adaptive)
        }
    }

    private fun formatCodecName(codecType: Int, config: Any?): String {
        var rawName = ""
        if (config != null) {
            try {
                val nameMethod = config.javaClass.getMethod("getCodecName")
                val name = nameMethod.invoke(config) as? String
                if (!name.isNullOrBlank()) {
                    rawName = name
                }
            } catch (_: Exception) {}

            if (rawName.isBlank()) {
                val str = config.toString()
                if (str.contains("Seamless", ignoreCase = true) ||
                    str.contains("Scalable", ignoreCase = true) ||
                    str.contains("SSC", ignoreCase = true)) {
                    rawName = "Samsung Seamless Codec"
                }
            }
        }

        if (rawName.contains("Seamless", ignoreCase = true) ||
            rawName.contains("Scalable", ignoreCase = true) ||
            rawName.contains("SSC", ignoreCase = true) ||
            rawName.contains("Samsung", ignoreCase = true)) {
            val sr = if (config != null) sampleRateMaskToHz(getSampleRateMask(config)) else 0
            val bps = if (config != null) bitsPerSampleMaskToBits(getBitsPerSampleMask(config)) else 0
            return when {
                sr >= 96000 || rawName.contains("UHQ", ignoreCase = true) -> "Samsung Seamless Codec (SSC UHQ)"
                bps > 16 || sr >= 48000 || rawName.contains("Hi-Fi", ignoreCase = true) -> "Samsung Seamless Codec (SSC Hi-Fi)"
                else -> "Samsung Seamless Codec (SSC)"
            }
        }

        if (rawName.isNotBlank()) return rawName

        return when (codecType) {
            CODEC_TYPE_LDAC -> "LDAC"
            CODEC_TYPE_APTX_HD -> "aptX HD"
            CODEC_TYPE_APTX -> "aptX"
            CODEC_TYPE_AAC -> "AAC"
            CODEC_TYPE_SBC -> "SBC"
            CODEC_TYPE_LC3 -> "LC3"
            CODEC_TYPE_OPUS -> "Opus"
            else -> "Bluetooth HD"
        }
    }

    private fun formatBitrateKbps(codecType: Int, codecSpecific1: Long, sampleRate: Int, config: Any?): Int {
        val name = formatCodecName(codecType, config)
        if (name.contains("SSC") || name.contains("Seamless") || name.contains("Scalable")) {
            return if (sampleRate >= 96000 || name.contains("UHQ")) 2304 else 512
        }
        return when (codecType) {
            CODEC_TYPE_LDAC -> {
                when (codecSpecific1) {
                    LDAC_QUALITY_HIGH -> if (sampleRate % 44100 == 0) 909 else 990
                    LDAC_QUALITY_MID -> if (sampleRate % 44100 == 0) 606 else 660
                    LDAC_QUALITY_LOW -> if (sampleRate % 44100 == 0) 303 else 330
                    else -> if (sampleRate % 44100 == 0) 909 else 990
                }
            }
            CODEC_TYPE_APTX_HD -> 576
            CODEC_TYPE_APTX -> if (sampleRate == 44100) 352 else 384
            CODEC_TYPE_AAC -> 256
            CODEC_TYPE_SBC -> 328
            CODEC_TYPE_LC3 -> 392
            else -> 600
        }
    }

    private fun formatBitrateDescription(codecType: Int, codecSpecific1: Long, bitrateKbps: Int, config: Any?): String {
        val name = formatCodecName(codecType, config)
        if (name.contains("SSC") || name.contains("Seamless") || name.contains("Scalable")) {
            return if (name.contains("UHQ") || bitrateKbps > 1000) {
                "UHQ dynamic (up to 24-bit/96kHz)"
            } else {
                "Scalable Hi-Fi (up to 512kbps)"
            }
        }
        return when (codecType) {
            CODEC_TYPE_LDAC -> {
                when (codecSpecific1) {
                    LDAC_QUALITY_HIGH -> "${bitrateKbps}kbps (High Quality)"
                    LDAC_QUALITY_MID -> "${bitrateKbps}kbps (Standard)"
                    LDAC_QUALITY_LOW -> "${bitrateKbps}kbps (Connection)"
                    LDAC_QUALITY_ADAPTIVE -> "Adaptive (up to ${bitrateKbps}kbps)"
                    else -> "${bitrateKbps}kbps"
                }
            }
            CODEC_TYPE_APTX_HD -> "576kbps"
            CODEC_TYPE_APTX -> "${bitrateKbps}kbps"
            CODEC_TYPE_AAC -> "~256kbps"
            CODEC_TYPE_SBC -> "~328kbps"
            CODEC_TYPE_LC3 -> "~392kbps"
            else -> "${bitrateKbps}kbps"
        }
    }

    private fun sampleRateMaskToHz(mask: Int): Int {
        return when {
            mask and SAMPLE_RATE_192000 != 0 -> 192000
            mask and SAMPLE_RATE_176400 != 0 -> 176400
            mask and SAMPLE_RATE_96000 != 0 -> 96000
            mask and SAMPLE_RATE_88200 != 0 -> 88200
            mask and SAMPLE_RATE_48000 != 0 -> 48000
            mask and SAMPLE_RATE_44100 != 0 -> 44100
            else -> 44100
        }
    }

    private fun highestSampleRateFromMask(mask: Int): Int = sampleRateMaskToHz(mask)

    /**
     * Selects a sample rate flag matching the source rate without EVER up-sampling.
     * If source is 44.1kHz, selects 44.1kHz even if the earbuds/phone support 96kHz or 192kHz.
     */
    fun selectSampleRateFlagNeverUpsample(mask: Int, sourceSampleRate: Int): Int {
        val target = if (sourceSampleRate > 0) sourceSampleRate else 44100

        // 1. Exact match if supported by device/codec
        when (target) {
            192000 -> if (mask and SAMPLE_RATE_192000 != 0) return SAMPLE_RATE_192000
            176400 -> if (mask and SAMPLE_RATE_176400 != 0) return SAMPLE_RATE_176400
            96000 -> if (mask and SAMPLE_RATE_96000 != 0) return SAMPLE_RATE_96000
            88200 -> if (mask and SAMPLE_RATE_88200 != 0) return SAMPLE_RATE_88200
            48000 -> if (mask and SAMPLE_RATE_48000 != 0) return SAMPLE_RATE_48000
            44100 -> if (mask and SAMPLE_RATE_44100 != 0) return SAMPLE_RATE_44100
        }

        // 2. If exact match is not available, pick highest supported rate that is STRICTLY <= target.
        // Prefer same sample rate family (44.1k vs 48k family) to avoid asynchronous resampling.
        if (target % 44100 == 0) {
            // 44.1kHz family (44.1k, 88.2k, 176.4k)
            if (target >= 176400 && mask and SAMPLE_RATE_176400 != 0) return SAMPLE_RATE_176400
            if (target >= 88200 && mask and SAMPLE_RATE_88200 != 0) return SAMPLE_RATE_88200
            if (mask and SAMPLE_RATE_44100 != 0) return SAMPLE_RATE_44100
        } else {
            // 48kHz family (48k, 96k, 192k)
            if (target >= 192000 && mask and SAMPLE_RATE_192000 != 0) return SAMPLE_RATE_192000
            if (target >= 96000 && mask and SAMPLE_RATE_96000 != 0) return SAMPLE_RATE_96000
            if (target >= 48000 && mask and SAMPLE_RATE_48000 != 0) return SAMPLE_RATE_48000
        }

        // General fallback: strictly <= target
        if (target >= 192000 && mask and SAMPLE_RATE_192000 != 0) return SAMPLE_RATE_192000
        if (target >= 176400 && mask and SAMPLE_RATE_176400 != 0) return SAMPLE_RATE_176400
        if (target >= 96000 && mask and SAMPLE_RATE_96000 != 0) return SAMPLE_RATE_96000
        if (target >= 88200 && mask and SAMPLE_RATE_88200 != 0) return SAMPLE_RATE_88200
        if (target >= 48000 && mask and SAMPLE_RATE_48000 != 0) return SAMPLE_RATE_48000
        if (target >= 44100 && mask and SAMPLE_RATE_44100 != 0) return SAMPLE_RATE_44100

        // Final fallback if hardware only supports higher than source: lowest available
        return if (mask and SAMPLE_RATE_44100 != 0) {
            SAMPLE_RATE_44100
        } else if (mask and SAMPLE_RATE_48000 != 0) {
            SAMPLE_RATE_48000
        } else {
            SAMPLE_RATE_44100
        }
    }

    private fun bitsPerSampleMaskToBits(mask: Int): Int {
        return when {
            mask and BITS_PER_SAMPLE_32 != 0 -> 32
            mask and BITS_PER_SAMPLE_24 != 0 -> 24
            mask and BITS_PER_SAMPLE_16 != 0 -> 16
            else -> 16
        }
    }

    private fun highestBitsPerSampleFromMask(mask: Int): Int = bitsPerSampleMaskToBits(mask)

    private fun selectHighestBitsPerSampleFlag(mask: Int): Int {
        return when {
            mask and BITS_PER_SAMPLE_32 != 0 -> BITS_PER_SAMPLE_32
            mask and BITS_PER_SAMPLE_24 != 0 -> BITS_PER_SAMPLE_24
            mask and BITS_PER_SAMPLE_16 != 0 -> BITS_PER_SAMPLE_16
            else -> BITS_PER_SAMPLE_16
        }
    }

    // Static JNI hooks called from Rust / exclusive_android.rs
    @JvmStatic
    fun getActiveBluetoothDescription(context: Context?): String? {
        val info = getBluetoothAudioInfo(context)
        return if (info.isConnected && info.summary.isNotBlank()) info.summary else null
    }

    @JvmStatic
    fun optimizeBluetoothAudio(context: Context?): String? {
        val info = optimizeForHighestQuality(context, 44100)
        return if (info.isConnected && info.summary.isNotBlank()) info.summary else null
    }

    @JvmStatic
    fun optimizeBluetoothAudioForRate(context: Context?, sourceSampleRate: Int): String? {
        val info = optimizeForHighestQuality(context, sourceSampleRate)
        return if (info.isConnected && info.summary.isNotBlank()) info.summary else null
    }
}
