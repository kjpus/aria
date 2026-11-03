package com.aria.player

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.media.AudioAttributes
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.os.PowerManager
import android.os.SystemClock
import android.support.v4.media.MediaBrowserCompat
import android.support.v4.media.MediaDescriptionCompat
import android.support.v4.media.MediaMetadataCompat
import android.support.v4.media.session.MediaSessionCompat
import android.support.v4.media.session.PlaybackStateCompat
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.media.MediaBrowserServiceCompat
import androidx.media.app.NotificationCompat as MediaNotificationCompat
import androidx.media.session.MediaButtonReceiver
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.concurrent.Executors

class AriaPlaybackService : MediaBrowserServiceCompat() {
    companion object {
        private const val TAG = "AriaPlaybackService"
        private const val NOTIFICATION_ID = 40401
        private const val CHANNEL_ID = "aria_playback_channel_v4"

        const val MEDIA_ID_ROOT = "root"
        const val MEDIA_ID_QUEUE = "queue"
        const val MEDIA_ID_ALBUMS = "albums"
        const val MEDIA_ID_PLAYLISTS = "playlists"
        const val MEDIA_ID_COMPOSERS = "composers"

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

        private val backgroundExecutor = Executors.newSingleThreadExecutor()

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

        // Automotive native functions
        @JvmStatic
        external fun nativeAutoEnsureAppCore(context: Context)

        @JvmStatic
        external fun nativeAutoGetQueue(): String

        @JvmStatic
        external fun nativeAutoGetAlbums(): String

        @JvmStatic
        external fun nativeAutoGetAlbumTracks(albumId: String): String

        @JvmStatic
        external fun nativeAutoGetPlaylists(): String

        @JvmStatic
        external fun nativeAutoGetPlaylistTracks(playlistId: String): String

        @JvmStatic
        external fun nativeAutoGetComposers(): String

        @JvmStatic
        external fun nativeAutoGetComposerAlbums(composer: String): String

        @JvmStatic
        external fun nativeAutoPlay()

        @JvmStatic
        external fun nativeAutoPause()

        @JvmStatic
        external fun nativeAutoPlayPause()

        @JvmStatic
        external fun nativeAutoNext()

        @JvmStatic
        external fun nativeAutoPrevious()

        @JvmStatic
        external fun nativeAutoStop()

        @JvmStatic
        external fun nativeAutoSeekTo(positionMs: Long)

        @JvmStatic
        external fun nativeAutoPlayQueueIndex(index: Int)

        @JvmStatic
        external fun nativeAutoPlayAlbum(albumId: String)

        @JvmStatic
        external fun nativeAutoPlayPlaylist(playlistId: String)

        @JvmStatic
        external fun nativeAutoPlayTrack(trackId: String, albumId: String?)

        @JvmStatic
        external fun nativeAutoPlayFromSearch(query: String)

        @JvmStatic
        external fun nativeAutoGetNowPlaying(): String?

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

        fun loadArtworkBitmap(path: String?, maxDimension: Int = 384): Bitmap? {
            if (path.isNullOrBlank()) return null
            return try {
                val file = File(path)
                if (!file.exists() || !file.canRead()) return null
                val boundsOptions = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                BitmapFactory.decodeFile(path, boundsOptions)
                val origWidth = boundsOptions.outWidth
                val origHeight = boundsOptions.outHeight
                if (origWidth <= 0 || origHeight <= 0) return null

                var sampleSize = 1
                while (origWidth / (sampleSize * 2) >= maxDimension && origHeight / (sampleSize * 2) >= maxDimension) {
                    sampleSize *= 2
                }
                val decodeOptions = BitmapFactory.Options().apply {
                    inSampleSize = sampleSize
                    inPreferredConfig = Bitmap.Config.RGB_565
                }
                BitmapFactory.decodeFile(path, decodeOptions)
            } catch (e: Throwable) {
                Log.w(TAG, "Failed to decode artwork from $path", e)
                null
            }
        }
    }

    private var wakeLock: PowerManager.WakeLock? = null
    private var wifiLock: WifiManager.WifiLock? = null
    private var mediaSession: MediaSessionCompat? = null
    private var currentTitle: String = "Aria"
    private var currentArtist: String = "Playing audio"
    private var currentAlbum: String = "Aria"
    private var currentAudioSpec: String = ""
    private var currentPositionMs: Long = 0L
    private var currentDurationMs: Long = 0L
    private var currentArtworkPath: String? = null
    private var currentCachedArtBitmap: Bitmap? = null
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

    override fun onCreate() {
        super.onCreate()
        instance = this
        isServiceRunning = true

        try {
            nativeRegisterContext(applicationContext)
        } catch (e: Throwable) {
            Log.w(TAG, "nativeRegisterContext failed", e)
        }

        try {
            nativeAutoEnsureAppCore(applicationContext)
        } catch (e: Throwable) {
            Log.w(TAG, "nativeAutoEnsureAppCore failed", e)
        }

        createNotificationChannel()
        initMediaSession()
        sessionToken = mediaSession?.sessionToken

        registerNoisyReceiver()
        registerAudioDeviceCallback()
        acquireLocks()

        syncWithNowPlaying()

        Log.i(TAG, "AriaPlaybackService created as MediaBrowserService")
    }

    override fun onBind(intent: Intent?): IBinder? {
        if (SERVICE_INTERFACE == intent?.action) {
            return super.onBind(intent)
        }
        return super.onBind(intent)
    }

    override fun onGetRoot(clientPackageName: String, clientUid: Int, rootHints: Bundle?): BrowserRoot? {
        Log.i(TAG, "onGetRoot called by $clientPackageName (uid=$clientUid)")
        val extras = Bundle().apply {
            putBoolean("android.service.media.extra.RECENT", true)
            putBoolean("android.service.media.extra.OFFLINE", true)
        }
        return BrowserRoot(MEDIA_ID_ROOT, extras)
    }

    override fun onLoadChildren(
        parentId: String,
        result: Result<MutableList<MediaBrowserCompat.MediaItem>>
    ) {
        result.detach()
        backgroundExecutor.execute {
            try {
                val items = mutableListOf<MediaBrowserCompat.MediaItem>()
                when (parentId) {
                    MEDIA_ID_ROOT -> {
                        items.add(
                            createCategoryItem(
                                MEDIA_ID_QUEUE,
                                "Current Queue",
                                "Now playing and queued tracks"
                            )
                        )
                        items.add(
                            createCategoryItem(
                                MEDIA_ID_ALBUMS,
                                "Albums",
                                "Classical albums in library"
                            )
                        )
                        items.add(
                            createCategoryItem(
                                MEDIA_ID_PLAYLISTS,
                                "Playlists",
                                "Saved playlists"
                            )
                        )
                        items.add(
                            createCategoryItem(
                                MEDIA_ID_COMPOSERS,
                                "Composers",
                                "Browse by classical composer"
                            )
                        )
                    }
                    MEDIA_ID_QUEUE -> {
                        val json = try {
                            nativeAutoGetQueue()
                        } catch (e: Throwable) {
                            Log.e(TAG, "nativeAutoGetQueue error", e)
                            "[]"
                        }
                        parseAndAddAutoItems(json, items, isPlayable = true)
                    }
                    MEDIA_ID_ALBUMS -> {
                        val json = try {
                            nativeAutoGetAlbums()
                        } catch (e: Throwable) {
                            Log.e(TAG, "nativeAutoGetAlbums error", e)
                            "[]"
                        }
                        parseAndAddAutoItems(json, items, isPlayable = false)
                    }
                    MEDIA_ID_PLAYLISTS -> {
                        val json = try {
                            nativeAutoGetPlaylists()
                        } catch (e: Throwable) {
                            Log.e(TAG, "nativeAutoGetPlaylists error", e)
                            "[]"
                        }
                        parseAndAddAutoItems(json, items, isPlayable = false)
                    }
                    MEDIA_ID_COMPOSERS -> {
                        val json = try {
                            nativeAutoGetComposers()
                        } catch (e: Throwable) {
                            Log.e(TAG, "nativeAutoGetComposers error", e)
                            "[]"
                        }
                        parseAndAddAutoItems(json, items, isPlayable = false)
                    }
                    else -> {
                        if (parentId.startsWith("album:")) {
                            val albumId = parentId.removePrefix("album:")
                            val json = try {
                                nativeAutoGetAlbumTracks(albumId)
                            } catch (e: Throwable) {
                                Log.e(TAG, "nativeAutoGetAlbumTracks error", e)
                                "[]"
                            }
                            parseAndAddAutoItems(json, items, isPlayable = true)
                        } else if (parentId.startsWith("playlist:")) {
                            val playlistId = parentId.removePrefix("playlist:")
                            val json = try {
                                nativeAutoGetPlaylistTracks(playlistId)
                            } catch (e: Throwable) {
                                Log.e(TAG, "nativeAutoGetPlaylistTracks error", e)
                                "[]"
                            }
                            parseAndAddAutoItems(json, items, isPlayable = true)
                        } else if (parentId.startsWith("composer:")) {
                            val composer = parentId.removePrefix("composer:")
                            val json = try {
                                nativeAutoGetComposerAlbums(composer)
                            } catch (e: Throwable) {
                                Log.e(TAG, "nativeAutoGetComposerAlbums error", e)
                                "[]"
                            }
                            parseAndAddAutoItems(json, items, isPlayable = false)
                        }
                    }
                }
                result.sendResult(items)
            } catch (e: Exception) {
                Log.e(TAG, "Error in onLoadChildren for parentId=$parentId", e)
                result.sendResult(mutableListOf())
            }
        }
    }

    private fun createCategoryItem(
        id: String,
        title: String,
        subtitle: String
    ): MediaBrowserCompat.MediaItem {
        val desc = MediaDescriptionCompat.Builder()
            .setMediaId(id)
            .setTitle(title)
            .setSubtitle(subtitle)
            .build()
        return MediaBrowserCompat.MediaItem(desc, MediaBrowserCompat.MediaItem.FLAG_BROWSABLE)
    }

    private fun parseAndAddAutoItems(
        json: String,
        items: MutableList<MediaBrowserCompat.MediaItem>,
        isPlayable: Boolean
    ) {
        val array = JSONArray(json)
        for (i in 0 until array.length()) {
            val obj = array.getJSONObject(i)
            val id = obj.optString("id")
            val title = obj.optString("title")
            val subtitle = obj.optString("subtitle")
            val artPath = if (obj.has("artworkPath") && !obj.isNull("artworkPath")) obj.getString("artworkPath") else null
            val durationMs = obj.optLong("durationMs", 0L)

            val descBuilder = MediaDescriptionCompat.Builder()
                .setMediaId(id)
                .setTitle(title)
                .setSubtitle(subtitle)

            if (!artPath.isNullOrBlank()) {
                val artFile = File(artPath)
                if (artFile.exists()) {
                    descBuilder.setIconUri(Uri.fromFile(artFile))
                }
            }

            val extras = Bundle().apply {
                if (durationMs > 0) {
                    putLong(MediaMetadataCompat.METADATA_KEY_DURATION, durationMs)
                }
                putInt("android.service.media.extra.DOWNLOAD_STATUS", 2)
            }
            descBuilder.setExtras(extras)

            val flag = if (isPlayable) {
                MediaBrowserCompat.MediaItem.FLAG_PLAYABLE
            } else {
                MediaBrowserCompat.MediaItem.FLAG_BROWSABLE
            }
            items.add(MediaBrowserCompat.MediaItem(descBuilder.build(), flag))
        }
    }

    private fun initMediaSession() {
        try {
            val session = MediaSessionCompat(this, "AriaPlaybackSession")
            session.setCallback(object : MediaSessionCompat.Callback() {
                override fun onPlay() {
                    triggerPlay()
                }

                override fun onPause() {
                    triggerPause()
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

                override fun onSeekTo(pos: Long) {
                    triggerSeekTo(pos)
                }

                override fun onPlayFromMediaId(mediaId: String?, extras: Bundle?) {
                    handlePlayFromMediaId(mediaId)
                }

                override fun onPlayFromSearch(query: String?, extras: Bundle?) {
                    handlePlayFromSearch(query)
                }

                override fun onMediaButtonEvent(mediaButtonEvent: Intent?): Boolean {
                    return super.onMediaButtonEvent(mediaButtonEvent)
                }
            })

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
            Log.w(TAG, "MediaSessionCompat initialization error", e)
        }
    }

    private fun handlePlayFromMediaId(mediaId: String?) {
        if (mediaId.isNullOrBlank()) return
        Log.i(TAG, "handlePlayFromMediaId: $mediaId")
        backgroundExecutor.execute {
            try {
                if (mediaId.startsWith("queue:")) {
                    val idx = mediaId.removePrefix("queue:").toIntOrNull() ?: 0
                    nativeAutoPlayQueueIndex(idx)
                } else if (mediaId.startsWith("album:")) {
                    val albumId = mediaId.removePrefix("album:")
                    nativeAutoPlayAlbum(albumId)
                } else if (mediaId.startsWith("playlist:")) {
                    val playlistId = mediaId.removePrefix("playlist:")
                    nativeAutoPlayPlaylist(playlistId)
                } else if (mediaId.startsWith("track:")) {
                    if (mediaId.contains(":album:")) {
                        val parts = mediaId.split(":album:")
                        val trackId = parts[0].removePrefix("track:")
                        val albumId = parts.getOrNull(1)
                        nativeAutoPlayTrack(trackId, albumId)
                    } else if (mediaId.contains(":playlist:")) {
                        val parts = mediaId.split(":playlist:")
                        val trackId = parts[0].removePrefix("track:")
                        nativeAutoPlayTrack(trackId, null)
                    } else {
                        val trackId = mediaId.removePrefix("track:")
                        nativeAutoPlayTrack(trackId, null)
                    }
                }
            } catch (e: Throwable) {
                Log.e(TAG, "handlePlayFromMediaId failed for $mediaId", e)
            }
        }
    }

    private fun handlePlayFromSearch(query: String?) {
        val q = query?.trim() ?: ""
        Log.i(TAG, "handlePlayFromSearch: '$q'")
        backgroundExecutor.execute {
            try {
                nativeAutoPlayFromSearch(q)
            } catch (e: Throwable) {
                Log.e(TAG, "handlePlayFromSearch failed for '$q'", e)
            }
        }
    }

    private fun triggerPlay() {
        try {
            nativeAutoPlay()
        } catch (e: Throwable) {
            Log.e(TAG, "nativeAutoPlay call failed", e)
        }
    }

    private fun triggerPause() {
        try {
            nativeAutoPause()
        } catch (e: Throwable) {
            Log.e(TAG, "nativeAutoPause call failed", e)
        }
    }

    private fun triggerPlayPause() {
        try {
            nativeAutoPlayPause()
        } catch (e: Throwable) {
            Log.e(TAG, "nativeAutoPlayPause call failed", e)
        }
    }

    private fun triggerNext() {
        try {
            nativeAutoNext()
        } catch (e: Throwable) {
            Log.e(TAG, "nativeAutoNext call failed", e)
        }
    }

    private fun triggerPrevious() {
        try {
            nativeAutoPrevious()
        } catch (e: Throwable) {
            Log.e(TAG, "nativeAutoPrevious call failed", e)
        }
    }

    private fun triggerStop() {
        try {
            nativeAutoStop()
        } catch (e: Throwable) {
            Log.e(TAG, "nativeAutoStop call failed", e)
        }
    }

    private fun triggerSeekTo(pos: Long) {
        try {
            nativeAutoSeekTo(pos)
        } catch (e: Throwable) {
            Log.e(TAG, "nativeAutoSeekTo call failed", e)
        }
    }

    private fun syncWithNowPlaying() {
        backgroundExecutor.execute {
            try {
                val jsonStr = nativeAutoGetNowPlaying()
                if (!jsonStr.isNullOrBlank()) {
                    val obj = JSONObject(jsonStr)
                    val title = obj.optString("title", currentTitle)
                    val artist = obj.optString("artist", currentArtist)
                    val album = obj.optString("album", currentAlbum)
                    val spec = obj.optString("spec", currentAudioSpec)
                    val pos = obj.optLong("positionMs", currentPositionMs)
                    val dur = obj.optLong("durationMs", currentDurationMs)
                    val playing = obj.optBoolean("isPlaying", isPlaying)
                    val artPath = if (obj.has("artworkPath") && !obj.isNull("artworkPath")) obj.getString("artworkPath") else null

                    if (!title.isNullOrBlank() && title != "Aria") currentTitle = title
                    if (!artist.isNullOrBlank() && artist != "Playing audio") currentArtist = artist
                    if (!album.isNullOrBlank()) currentAlbum = album
                    if (!spec.isNullOrBlank()) currentAudioSpec = spec
                    currentPositionMs = pos
                    currentDurationMs = dur
                    isPlaying = playing

                    if (artPath != currentArtworkPath) {
                        currentArtworkPath = artPath
                        currentCachedArtBitmap = loadArtworkBitmap(artPath)
                    }

                    updateSessionAndNotification()
                }
            } catch (e: Throwable) {
                Log.w(TAG, "syncWithNowPlaying error", e)
            }
        }
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
            nativeAutoStop()
        } catch (e: Throwable) {
            Log.e(TAG, "nativeAutoStop failed", e)
        }
        updateSessionAndNotification()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        MediaButtonReceiver.handleIntent(mediaSession, intent)

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

                syncWithNowPlaying()
                updateSessionAndNotification()
            }
        }
        return START_NOT_STICKY
    }

    private fun updateSessionAndNotification() {
        val session = mediaSession
        if (session != null) {
            val state = if (isPlaying) {
                PlaybackStateCompat.STATE_PLAYING
            } else {
                PlaybackStateCompat.STATE_PAUSED
            }
            val stateBuilder = PlaybackStateCompat.Builder()
                .setActions(
                    PlaybackStateCompat.ACTION_PLAY or
                        PlaybackStateCompat.ACTION_PAUSE or
                        PlaybackStateCompat.ACTION_PLAY_PAUSE or
                        PlaybackStateCompat.ACTION_SKIP_TO_NEXT or
                        PlaybackStateCompat.ACTION_SKIP_TO_PREVIOUS or
                        PlaybackStateCompat.ACTION_SEEK_TO or
                        PlaybackStateCompat.ACTION_STOP
                )
                .setState(
                    state,
                    currentPositionMs,
                    1.0f,
                    SystemClock.elapsedRealtime()
                )
            session.setPlaybackState(stateBuilder.build())

            val contentText = if (currentAudioSpec.isNotBlank()) {
                "$currentArtist • $currentAudioSpec"
            } else {
                currentArtist
            }

            val metaBuilder = MediaMetadataCompat.Builder()
                .putString(MediaMetadataCompat.METADATA_KEY_TITLE, currentTitle)
                .putString(MediaMetadataCompat.METADATA_KEY_ARTIST, contentText)
                .putString(MediaMetadataCompat.METADATA_KEY_ALBUM, currentAlbum)
                .putString(MediaMetadataCompat.METADATA_KEY_ALBUM_ARTIST, currentArtist)
                .putString(MediaMetadataCompat.METADATA_KEY_DISPLAY_TITLE, currentTitle)
                .putString(MediaMetadataCompat.METADATA_KEY_DISPLAY_SUBTITLE, contentText)
                .putString(MediaMetadataCompat.METADATA_KEY_DISPLAY_DESCRIPTION, currentAudioSpec)

            if (currentDurationMs > 0) {
                metaBuilder.putLong(MediaMetadataCompat.METADATA_KEY_DURATION, currentDurationMs)
            }

            val art = currentCachedArtBitmap
            if (art != null) {
                metaBuilder.putBitmap(MediaMetadataCompat.METADATA_KEY_ALBUM_ART, art)
                metaBuilder.putBitmap(MediaMetadataCompat.METADATA_KEY_DISPLAY_ICON, art)
            }

            session.setMetadata(metaBuilder.build())
        }

        try {
            notifyChildrenChanged(MEDIA_ID_QUEUE)
        } catch (_: Exception) {}

        val notification = buildNotification()
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

    private fun buildNotification(): Notification {
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

        val contentText = if (currentAudioSpec.isNotBlank()) {
            "$currentArtist • $currentAudioSpec"
        } else {
            currentArtist
        }

        val builder = NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle(currentTitle)
            .setContentText(contentText)
            .setSmallIcon(R.drawable.ic_player_notification)
            .setOngoing(isPlaying)
            .setOnlyAlertOnce(true)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setContentIntent(contentPendingIntent)
            .setCategory(NotificationCompat.CATEGORY_TRANSPORT)
            .addAction(
                NotificationCompat.Action.Builder(
                    android.R.drawable.ic_media_previous,
                    "Previous",
                    prevPendingIntent
                ).build()
            )
            .addAction(
                NotificationCompat.Action.Builder(
                    playPauseIcon,
                    playPauseTitle,
                    playPausePendingIntent
                ).build()
            )
            .addAction(
                NotificationCompat.Action.Builder(
                    android.R.drawable.ic_media_next,
                    "Next",
                    nextPendingIntent
                ).build()
            )
            .addAction(
                NotificationCompat.Action.Builder(
                    R.drawable.ic_vol_down,
                    "Vol -",
                    volDownPendingIntent
                ).build()
            )
            .addAction(
                NotificationCompat.Action.Builder(
                    R.drawable.ic_vol_up,
                    "Vol +",
                    volUpPendingIntent
                ).build()
            )

        if (currentAudioSpec.isNotBlank()) {
            builder.setSubText(currentAudioSpec)
        }

        val art = currentCachedArtBitmap
        if (art != null) {
            builder.setLargeIcon(art)
        }

        mediaSession?.let { session ->
            val mediaStyle = MediaNotificationCompat.MediaStyle()
                .setMediaSession(session.sessionToken)
                .setShowActionsInCompactView(0, 1, 2)
            builder.setStyle(mediaStyle)
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
