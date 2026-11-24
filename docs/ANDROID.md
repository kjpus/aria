# Android And Android Auto

Aria ships an Android build powered by Tauri 2 Mobile, a custom Kotlin audio/media service layer, and JNI bridges into the Rust `AppCore` and `PlaybackService`.

## Important: Checked-In Custom Kotlin Code

> [!IMPORTANT]
> `src-tauri/gen/android/app/src/main/java/com/aria/player/` contains **custom, hand-maintained Kotlin code** checked into version control. Do **not** blindly run `tauri android init` with overwrite enabled, or you will lose the Android playback, USB DAC, Bluetooth HD, and Android Auto implementations.

Custom Android sources:

- `MainActivity.kt` — WebView host, OAuth browser launcher (`openBrowser` with strict `http`/`https` validation), USB/Bluetooth helper lifecycle, and volume/key routing.
- `AriaPlaybackService.kt` — `MediaBrowserServiceCompat` + `MediaSessionCompat` foreground service for background playback, lock-screen controls, and **Android Auto** library browsing.
- `AriaBitPerfectPlayer.kt` — Native `AudioTrack` output supporting Android 14+ USB DAC `MIXER_BEHAVIOR_BIT_PERFECT`, hi-res PCM rates (up to 384 kHz / 32-bit with 64-bit buffer math), and gapless pre-warming.
- `AriaBluetoothAudioHelper.kt` — Bluetooth A2DP/HD codec inspection and device routing (with Android 12+ `BLUETOOTH_CONNECT` `SecurityException` guards).

Rust JNI bridges:

- `src-tauri/src/auto_jni.rs` — Android Auto browse tree, play-from-media-id, search, transport actions, and `AppCore` bootstrap.
- `crates/playback/src/exclusive_android.rs` — Symphonia decode, `rubato` resampling, safe unaligned-free PCM volume scaling, seek-offset position tracking, and JNI calls into `AriaBitPerfectPlayer`.

## Prerequisites

1. **Android SDK & NDK** (compileSdk / targetSdk `36`, minSdk `26`, NDK r26+).
2. **Environment variables:** `ANDROID_HOME` and `NDK_HOME` configured on `PATH`.
3. **Rust Android targets:**

```powershell
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android
```

## 16 KB Page-Size Alignment

Android 15+ devices with 16 KB memory pages require native `.so` libraries linked with `-Wl,-z,max-page-size=16384`.

Both `.cargo/config.toml` (workspace root) and `src-tauri/.cargo/config.toml` (for commands invoked inside `src-tauri/`) configure this linker flag for all four Android ABI targets. Keep both files synchronized if target flags change.

## Threading & Lifecycle Rules

- `AriaPlaybackService` must **never** call `nativeAutoEnsureAppCore()` synchronously on the main UI thread (SQLite open/migrations can trigger an ANR); initialization runs on `backgroundExecutor`, while notification and `MediaSession` updates post back to `mainHandler`.
- Because `startForegroundService()` is used on Android 8+, `onStartCommand()` guarantees `startForeground()` is invoked on initial service start before detaching/stopping when not playing (`hasEnteredForeground`).
- `MainActivity.onDestroy()` must not tear down `AriaBluetoothAudioHelper` while `AriaPlaybackService.isServiceRunning` is `true`, so background playback keeps its Bluetooth proxy active.

## Building & Signing

Development build / run on a connected device or emulator:

```powershell
npm.cmd run tauri -- android dev
```

Release APK / AAB build:

```powershell
npm.cmd run tauri -- android build
```

### Release Keystore Signing

`src-tauri/gen/android/app/build.gradle.kts` reads release signing credentials from either:

1. `src-tauri/gen/android/keystore.properties` (git-ignored, never commit):
   ```properties
   storeFile=/path/to/aria-release.jks
   storePassword=...
   keyAlias=...
   keyPassword=...
   ```
2. Environment variables:
   - `ARIA_KEYSTORE_PATH`
   - `ARIA_KEYSTORE_PASSWORD`
   - `ARIA_KEY_ALIAS`
   - `ARIA_KEY_PASSWORD`

If neither is configured, the Gradle `release` build type falls back to the debug signing config for local testing. Always supply a release keystore before publishing.

## Testing Android Auto

1. Install the debug or release APK on an Android device or emulator.
2. Enable **Unknown sources** in Android Auto developer settings.
3. Connect to Google's **Desktop Head Unit (DHU)** via `adb forward tcp:5277 tcp:5277` and verify library browsing (Albums, Playlists, Queue), playback controls, and background service continuity.
