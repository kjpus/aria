package com.aria.player

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.media.AudioAttributes
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.media.MediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.net.wifi.WifiManager
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import android.util.Log

class AriaPlaybackService : Service() {
    companion object {
        private const val TAG = "AriaPlaybackService"
        private const val NOTIFICATION_ID = 40401
        private const val CHANNEL_ID = "aria_playback_channel_v4"

        const val ACTION_START = "com.aria.player.action.START"
        const val ACTION_UPDATE = "com.aria.player.action.UPDATE"
        const val ACTION_STOP = "com.aria.player.action.STOP"
        const val ACTION_PLAY_PAUSE = "com.aria.player.action.PLAY_PAUSE"
        const val ACTION_PREVIOUS = "com.aria.player.action.PREVIOUS"
        const val ACTION_NEXT = "com.aria.player.action.NEXT"
        const val ACTION_VOLUME_DOWN = "com.aria.player.action.VOLUME_DOWN"
        const val ACTION_VOLUME_UP = "com.aria.player.action.VOLUME_UP"

        const val EXTRA_TITLE = "extra_title"
        const val EXTRA_ARTIST = "extra_artist"
        const val EXTRA_AUDIO_SPEC = "extra_audio_spec"
        const val EXTRA_IS_PLAYING = "extra_is_playing"

        @Volatile
        var instance: AriaPlaybackService? = null
            private set

        @Volatile
        var isServiceRunning = false
            private set

        init {
            try {
                System.loadLibrary("aria_tauri")
            } catch (e: UnsatisfiedLinkError) {
                Log.w(TAG, "libaria_tauri already loaded or pending", e)
            }
        }

        @JvmStatic
        external fun nativeRegisterContext(context: Context)

        @JvmStatic
        external fun nativeLogDiagnostic(level: Int, subsystem: String, message: String)

        @JvmStatic
        fun logDiagnostic(level: Int, subsystem: String, message: String) {
            try {
                nativeLogDiagnostic(level, subsystem, message)
            } catch (_: Throwable) {
                when (level) {
                    4 -> Log.e("Aria:$subsystem", message)
                    3 -> Log.w("Aria:$subsystem", message)
                    2 -> Log.i("Aria:$subsystem", message)
                    else -> Log.d("Aria:$subsystem", message)
                }
            }
        }

        @JvmStatic
        private external fun nativePlayPause()

        @JvmStatic
        private external fun nativeNext()

        @JvmStatic
        private external fun nativePrevious()

        @JvmStatic
        private external fun nativeStopPlayback()

        @JvmStatic
        fun hasExternalAudioOutput(context: Context): Boolean {
            val am = context.getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: return false
            val devices = am.getDevices(AudioManager.GET_DEVICES_OUTPUTS)
            return devices.any { d ->
                when (d.type) {
                    AudioDeviceInfo.TYPE_BLUETOOTH_A2DP,
                    AudioDeviceInfo.TYPE_BLUETOOTH_SCO,
                    AudioDeviceInfo.TYPE_BLE_HEADSET,
                    AudioDeviceInfo.TYPE_BLE_SPEAKER,
                    AudioDeviceInfo.TYPE_BLE_BROADCAST,
                    AudioDeviceInfo.TYPE_USB_DEVICE,
                    AudioDeviceInfo.TYPE_USB_HEADSET,
                    AudioDeviceInfo.TYPE_USB_ACCESSORY,
                    AudioDeviceInfo.TYPE_WIRED_HEADSET,
                    AudioDeviceInfo.TYPE_WIRED_HEADPHONES,
                    AudioDeviceInfo.TYPE_HEARING_AID,
                    AudioDeviceInfo.TYPE_LINE_ANALOG,
                    AudioDeviceInfo.TYPE_LINE_DIGITAL -> true
                    else -> false
                }
            }
        }

        @JvmStatic
        fun onAudioReroutedToSpeaker() {
            instance?.handleAudioBecomingNoisy()
        }

        @JvmStatic
        fun startPlayback(context: Context, title: String?, artist: String?) {
            startPlayback(context, title, artist, null)
        }

        @JvmStatic
        fun startPlayback(context: Context, title: String?, artist: String?, audioSpec: String?) {
            val intent = Intent(context, AriaPlaybackService::class.java).apply {
                action = ACTION_START
                putExtra(EXTRA_TITLE, title)
                putExtra(EXTRA_ARTIST, artist)
                putExtra(EXTRA_AUDIO_SPEC, audioSpec)
                putExtra(EXTRA_IS_PLAYING, true)
            }
            try {
                if (isServiceRunning) {
                    context.startService(intent)
                } else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                    try {
                        context.startForegroundService(intent)
                    } catch (e: Exception) {
                        Log.w(TAG, "startForegroundService rejected, attempting startService", e)
                        context.startService(intent)
                    }
                } else {
                    context.startService(intent)
                }
            } catch (e: Exception) {
                Log.e(TAG, "Failed to start AriaPlaybackService", e)
            }
        }

        @JvmStatic
        fun updatePlayback(context: Context, title: String?, artist: String?, isPlaying: Boolean) {
            updatePlayback(context, title, artist, null, isPlaying)
        }

        @JvmStatic
        fun updatePlayback(context: Context, title: String?, artist: String?, audioSpec: String?, isPlaying: Boolean) {
            val intent = Intent(context, AriaPlaybackService::class.java).apply {
                action = ACTION_UPDATE
                putExtra(EXTRA_TITLE, title)
                putExtra(EXTRA_ARTIST, artist)
                putExtra(EXTRA_AUDIO_SPEC, audioSpec)
                putExtra(EXTRA_IS_PLAYING, isPlaying)
            }
            try {
                if (isServiceRunning) {
                    context.startService(intent)
                } else if (isPlaying) {
                    startPlayback(context, title, artist, audioSpec)
                }
            } catch (e: Exception) {
                Log.e(TAG, "Failed to update AriaPlaybackService", e)
            }
        }

        @JvmStatic
        fun stopPlayback(context: Context) {
            val intent = Intent(context, AriaPlaybackService::class.java).apply {
                action = ACTION_STOP
            }
            try {
                context.startService(intent)
            } catch (e: Exception) {
                Log.e(TAG, "Failed to stop AriaPlaybackService", e)
            }
        }
    }

    private var wakeLock: PowerManager.WakeLock? = null
    private var wifiLock: WifiManager.WifiLock? = null
    private var mediaSession: MediaSession? = null
    private var currentTitle: String = "Aria"
    private var currentArtist: String = "Playing audio"
    private var currentAudioSpec: String = ""
    private var isPlaying: Boolean = false
    private var noisyReceiverRegistered = false
    private var audioDeviceCallback: AudioDeviceCallback? = null

    private val noisyReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if (intent?.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) {
                Log.i(TAG, "ACTION_AUDIO_BECOMING_NOISY received -> stopping playback")
                handleAudioBecomingNoisy()
            }
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        instance = this
        isServiceRunning = true
        try {
            nativeRegisterContext(applicationContext)
        } catch (e: Throwable) {
            Log.w(TAG, "nativeRegisterContext failed", e)
        }
        createNotificationChannel()
        initMediaSession()
        registerNoisyReceiver()
        registerAudioDeviceCallback()
        acquireLocks()
        Log.i(TAG, "AriaPlaybackService created")
    }

    private fun registerNoisyReceiver() {
        if (!noisyReceiverRegistered) {
            val filter = IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                registerReceiver(noisyReceiver, filter, Context.RECEIVER_NOT_EXPORTED)
            } else {
                registerReceiver(noisyReceiver, filter)
            }
            noisyReceiverRegistered = true
        }
    }

    private fun unregisterNoisyReceiver() {
        if (noisyReceiverRegistered) {
            try {
                unregisterReceiver(noisyReceiver)
            } catch (_: Exception) {}
            noisyReceiverRegistered = false
        }
    }

    private fun registerAudioDeviceCallback() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            val audioManager = getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: return
            val callback = object : AudioDeviceCallback() {
                override fun onAudioDevicesRemoved(removedDevices: Array<out AudioDeviceInfo>?) {
                    checkAndStopIfOnlySpeakerRemaining()
                }
            }
            audioManager.registerAudioDeviceCallback(callback, null)
            this.audioDeviceCallback = callback
        }
    }

    private fun unregisterAudioDeviceCallback() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M && audioDeviceCallback != null) {
            val audioManager = getSystemService(Context.AUDIO_SERVICE) as? AudioManager
            audioManager?.unregisterAudioDeviceCallback(audioDeviceCallback)
            audioDeviceCallback = null
        }
    }

    fun checkAndStopIfOnlySpeakerRemaining() {
        if (isPlaying && !hasExternalAudioOutput(this)) {
            Log.i(TAG, "External audio device disconnected and only speaker remains -> stopping playback")
            handleAudioBecomingNoisy()
        }
    }

    fun handleAudioBecomingNoisy() {
        Log.i(TAG, "Silencing and stopping playback due to audio output disconnect")
        isPlaying = false
        try {
            AriaBitPerfectPlayer.silenceActivePlayback()
        } catch (e: Throwable) {
            Log.w(TAG, "silenceActivePlayback failed", e)
        }
        try {
            nativeStopPlayback()
        } catch (e: Throwable) {
            Log.e(TAG, "nativeStopPlayback failed", e)
        }
        updatePlayback(this, currentTitle, currentArtist, currentAudioSpec, false)
    }

    private fun initMediaSession() {
        try {
            val session = MediaSession(this, "AriaPlaybackSession")
            session.setCallback(object : MediaSession.Callback() {
                override fun onPlay() {
                    triggerPlayPause()
                }

                override fun onPause() {
                    triggerPlayPause()
                }

                override fun onSkipToNext() {
                    triggerNext()
                }

                override fun onSkipToPrevious() {
                    triggerPrevious()
                }

                override fun onStop() {
                    triggerStop()
                }
            })
            val audioAttributes = AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                .build()
            session.setPlaybackToLocal(audioAttributes)
            session.setFlags(
                MediaSession.FLAG_HANDLES_MEDIA_BUTTONS or
                    MediaSession.FLAG_HANDLES_TRANSPORT_CONTROLS
            )

            val launchIntent = packageManager.getLaunchIntentForPackage(packageName) ?: Intent(this, MainActivity::class.java)
            launchIntent.flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
            val sessionActivityPendingIntent = PendingIntent.getActivity(
                this,
                0,
                launchIntent,
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
            )
            session.setSessionActivity(sessionActivityPendingIntent)

            session.isActive = true
            mediaSession = session
        } catch (e: Exception) {
            Log.w(TAG, "MediaSession initialization error", e)
        }
    }

    private fun triggerStop() {
        try {
            nativeStopPlayback()
        } catch (e: Throwable) {
            Log.e(TAG, "nativeStopPlayback call failed", e)
        }
    }

    private fun triggerPlayPause() {
        try {
            nativePlayPause()
        } catch (e: Throwable) {
            Log.e(TAG, "nativePlayPause call failed", e)
        }
    }

    private fun triggerNext() {
        try {
            nativeNext()
        } catch (e: Throwable) {
            Log.e(TAG, "nativeNext call failed", e)
        }
    }

    private fun triggerPrevious() {
        try {
            nativePrevious()
        } catch (e: Throwable) {
            Log.e(TAG, "nativePrevious call failed", e)
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val action = intent?.action ?: ACTION_START
        when (action) {
            ACTION_PLAY_PAUSE -> {
                triggerPlayPause()
                return START_NOT_STICKY
            }
            ACTION_PREVIOUS -> {
                triggerPrevious()
                return START_NOT_STICKY
            }
            ACTION_NEXT -> {
                triggerNext()
                return START_NOT_STICKY
            }
            ACTION_VOLUME_DOWN -> {
                val audioManager = getSystemService(Context.AUDIO_SERVICE) as? AudioManager
                audioManager?.adjustStreamVolume(
                    AudioManager.STREAM_MUSIC,
                    AudioManager.ADJUST_LOWER,
                    AudioManager.FLAG_SHOW_UI
                )
                return START_NOT_STICKY
            }
            ACTION_VOLUME_UP -> {
                val audioManager = getSystemService(Context.AUDIO_SERVICE) as? AudioManager
                audioManager?.adjustStreamVolume(
                    AudioManager.STREAM_MUSIC,
                    AudioManager.ADJUST_RAISE,
                    AudioManager.FLAG_SHOW_UI
                )
                return START_NOT_STICKY
            }
            ACTION_STOP -> {
                Log.i(TAG, "Stopping playback service")
                isPlaying = false
                releaseLocks()
                mediaSession?.isActive = false
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
                    stopForeground(STOP_FOREGROUND_REMOVE)
                } else {
                    @Suppress("DEPRECATION")
                    stopForeground(true)
                }
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_START, ACTION_UPDATE -> {
                val newTitle = intent?.getStringExtra(EXTRA_TITLE)
                val newArtist = intent?.getStringExtra(EXTRA_ARTIST)
                val newAudioSpec = intent?.getStringExtra(EXTRA_AUDIO_SPEC)
                val newIsPlaying = intent?.getBooleanExtra(EXTRA_IS_PLAYING, true) ?: true

                if (!newTitle.isNullOrBlank() && newTitle != "Aria") {
                    currentTitle = newTitle
                }
                if (!newArtist.isNullOrBlank() && newArtist != "Playing audio") {
                    currentArtist = newArtist
                }
                if (!newAudioSpec.isNullOrBlank()) {
                    currentAudioSpec = newAudioSpec
                }
                isPlaying = newIsPlaying

                if (isPlaying) {
                    acquireLocks()
                } else {
                    releaseWakeLock()
                }

                val notification = buildNotification(currentTitle, currentArtist, currentAudioSpec, isPlaying)
                val notificationManager = getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager
                try {
                    if (isPlaying) {
                        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                            startForeground(
                                NOTIFICATION_ID,
                                notification,
                                ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK
                            )
                        } else {
                            startForeground(NOTIFICATION_ID, notification)
                        }
                    } else {
                        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
                            stopForeground(STOP_FOREGROUND_DETACH)
                        } else {
                            @Suppress("DEPRECATION")
                            stopForeground(false)
                        }
                        notificationManager?.notify(NOTIFICATION_ID, notification)
                    }
                } catch (e: Exception) {
                    Log.e(TAG, "Notification / Foreground state error", e)
                }
            }
        }
        return START_NOT_STICKY
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val manager = getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager
            try {
                manager?.deleteNotificationChannel("aria_playback_channel")
                manager?.deleteNotificationChannel("aria_playback_channel_v2")
                manager?.deleteNotificationChannel("aria_playback_channel_v3")
            } catch (_: Exception) {}

            val channel = NotificationChannel(
                CHANNEL_ID,
                "Aria Audio Playback",
                NotificationManager.IMPORTANCE_DEFAULT
            ).apply {
                description = "Shows currently playing track and controls background playback"
                setSound(null, null)
                enableVibration(false)
                setShowBadge(true)
                lockscreenVisibility = Notification.VISIBILITY_PUBLIC
            }
            manager?.createNotificationChannel(channel)
        }
    }

    private fun buildNotification(title: String, artist: String, audioSpec: String, isPlaying: Boolean): Notification {
        val launchIntent = packageManager.getLaunchIntentForPackage(packageName) ?: Intent(this, MainActivity::class.java)
        launchIntent.flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        val contentPendingIntent = PendingIntent.getActivity(
            this,
            0,
            launchIntent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )

        fun createServicePendingIntent(action: String, requestCode: Int): PendingIntent {
            val intent = Intent(this, AriaPlaybackService::class.java).apply {
                this.action = action
            }
            return PendingIntent.getService(
                this,
                requestCode,
                intent,
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
            )
        }

        val prevPendingIntent = createServicePendingIntent(ACTION_PREVIOUS, 1)
        val playPausePendingIntent = createServicePendingIntent(ACTION_PLAY_PAUSE, 2)
        val nextPendingIntent = createServicePendingIntent(ACTION_NEXT, 3)
        val volDownPendingIntent = createServicePendingIntent(ACTION_VOLUME_DOWN, 4)
        val volUpPendingIntent = createServicePendingIntent(ACTION_VOLUME_UP, 5)

        val playPauseIcon = if (isPlaying) {
            android.R.drawable.ic_media_pause
        } else {
            android.R.drawable.ic_media_play
        }
        val playPauseTitle = if (isPlaying) "Pause" else "Play"

        val contentText = if (audioSpec.isNotBlank()) {
            "$artist • $audioSpec"
        } else {
            artist
        }

        val builder = Notification.Builder(this, CHANNEL_ID)
            .setContentTitle(title)
            .setContentText(contentText)
            .setSmallIcon(R.drawable.ic_player_notification)
            .setOngoing(isPlaying)
            .setOnlyAlertOnce(true)
            .setVisibility(Notification.VISIBILITY_PUBLIC)
            .setContentIntent(contentPendingIntent)
            .setCategory(Notification.CATEGORY_TRANSPORT)
            .addAction(
                Notification.Action.Builder(
                    android.R.drawable.ic_media_previous,
                    "Previous",
                    prevPendingIntent
                ).build()
            )
            .addAction(
                Notification.Action.Builder(
                    playPauseIcon,
                    playPauseTitle,
                    playPausePendingIntent
                ).build()
            )
            .addAction(
                Notification.Action.Builder(
                    android.R.drawable.ic_media_next,
                    "Next",
                    nextPendingIntent
                ).build()
            )
            .addAction(
                Notification.Action.Builder(
                    R.drawable.ic_vol_down,
                    "Vol -",
                    volDownPendingIntent
                ).build()
            )
            .addAction(
                Notification.Action.Builder(
                    R.drawable.ic_vol_up,
                    "Vol +",
                    volUpPendingIntent
                ).build()
            )

        if (audioSpec.isNotBlank()) {
            builder.setSubText(audioSpec)
        }

        mediaSession?.let { session ->
            val stateBuilder = PlaybackState.Builder()
                .setActions(
                    PlaybackState.ACTION_PLAY or
                        PlaybackState.ACTION_PAUSE or
                        PlaybackState.ACTION_PLAY_PAUSE or
                        PlaybackState.ACTION_SKIP_TO_NEXT or
                        PlaybackState.ACTION_SKIP_TO_PREVIOUS or
                        PlaybackState.ACTION_STOP
                )
                .setState(
                    if (isPlaying) PlaybackState.STATE_PLAYING else PlaybackState.STATE_PAUSED,
                    PlaybackState.PLAYBACK_POSITION_UNKNOWN,
                    1.0f
                )
            session.setPlaybackState(stateBuilder.build())

            val mediaMetadata = MediaMetadata.Builder()
                .putString(MediaMetadata.METADATA_KEY_TITLE, title)
                .putString(MediaMetadata.METADATA_KEY_ARTIST, contentText)
                .putString(MediaMetadata.METADATA_KEY_ALBUM, if (audioSpec.isNotBlank()) audioSpec else "Aria")
                .putString(MediaMetadata.METADATA_KEY_ALBUM_ARTIST, artist)
                .putString(MediaMetadata.METADATA_KEY_DISPLAY_TITLE, title)
                .putString(MediaMetadata.METADATA_KEY_DISPLAY_SUBTITLE, contentText)
                .putString(MediaMetadata.METADATA_KEY_DISPLAY_DESCRIPTION, audioSpec)
                .build()
            session.setMetadata(mediaMetadata)

            val mediaStyle = Notification.MediaStyle()
                .setMediaSession(session.sessionToken)
                .setShowActionsInCompactView(0, 1, 2)
            builder.style = mediaStyle
        }

        return builder.build()
    }

    private fun acquireLocks() {
        try {
            if (wakeLock == null) {
                val pm = getSystemService(Context.POWER_SERVICE) as? PowerManager
                wakeLock = pm?.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "Aria:PlaybackWakeLock")?.apply {
                    setReferenceCounted(false)
                }
            }
            wakeLock?.let {
                if (!it.isHeld) {
                    it.acquire(24 * 60 * 60 * 1000L) // 24 hour safety ceiling
                }
            }
        } catch (e: Exception) {
            Log.w(TAG, "Could not acquire WakeLock", e)
        }

        try {
            if (wifiLock == null) {
                val wm = applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
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
                    logDiagnostic(2, "Playback", "Acquired WifiLock (mode=${if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) "LOW_LATENCY" else "HIGH_PERF"})")
                }
            }
        } catch (e: Exception) {
            Log.w(TAG, "Could not acquire WifiLock", e)
        }
    }

    private fun releaseWakeLock() {
        try {
            wakeLock?.let {
                if (it.isHeld) it.release()
            }
            wakeLock = null
        } catch (e: Exception) {
            Log.w(TAG, "Error releasing WakeLock", e)
        }
    }

    private fun releaseLocks() {
        releaseWakeLock()
        try {
            wifiLock?.let {
                if (it.isHeld) it.release()
            }
            wifiLock = null
        } catch (e: Exception) {
            Log.w(TAG, "Error releasing WifiLock", e)
        }
    }

    override fun onDestroy() {
        if (instance === this) {
            instance = null
        }
        isServiceRunning = false
        unregisterNoisyReceiver()
        unregisterAudioDeviceCallback()
        mediaSession?.isActive = false
        mediaSession?.release()
        mediaSession = null
        releaseLocks()
        Log.i(TAG, "AriaPlaybackService destroyed")
        super.onDestroy()
    }
}
