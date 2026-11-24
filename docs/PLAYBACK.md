# Playback Architecture

Aria treats playback as a long-lived Rust service (`crates/playback/src/service.rs`) rather than stateless UI callbacks. The backend owns the transport state, queue, position clock, output device selection, and active audio backend.

## Output Paths

| Platform | Output Mode | Implementation | Notes |
| --- | --- | --- | --- |
| Windows | Shared | `rodio` (`cpal` WASAPI shared) | System mixer; never labeled bit-perfect |
| Windows | Exclusive (WASAPI) | `crates/playback/src/exclusive_wasapi.rs` | Event-driven `AUDCLNT_SHAREMODE_EXCLUSIVE` with `symphonia` decode + optional `rubato` resampling |
| macOS / Linux | Shared | `rodio` (CoreAudio / ALSA) | Standard system mixer output |
| Android | USB DAC Bit-Perfect | `exclusive_android.rs` + `AriaBitPerfectPlayer.kt` | Android 14+ `AudioMixerAttributes` `MIXER_BEHAVIOR_BIT_PERFECT` when supported by USB DAC |
| Android | Bluetooth HD / System | `exclusive_android.rs` + `AriaBluetoothAudioHelper.kt` | Direct `AudioTrack` output with per-device Bluetooth/USB classification and gapless pre-warming |

## Resampling Policy

Implemented in `crates/playback/src/resample.rs` using `rubato::FftFixedIn`:

- When the hardware output supports the track's native sample rate, playback bypasses resampling (`is_resampling() == false`).
- When the device does not support the source rate, `select_resample_rate` chooses the closest compatible higher rate (preferring upsampling over downsampling, falling back to the highest supported lower rate if needed).
- The resampler reuses its internal `temp_out` scratch buffer across blocks to avoid per-chunk heap allocations and drains the filter's `output_delay()` tail frames on `flush()`.

## Real-Time Audio Safety Invariants

- **No unaligned pointer casts:** PCM volume scaling in both `exclusive_wasapi.rs` and `exclusive_android.rs` operates on `chunks_exact_mut(2)` / `chunks_exact_mut(4)` with `from_ne_bytes`/`to_ne_bytes` rather than casting `&mut [u8]` to typed sample slices.
- **No blocking while holding WASAPI render buffers:** `exclusive_wasapi.rs` stages decoded and volume-scaled bytes into a local buffer *before* calling `IAudioRenderClient::GetBuffer`, checks `GetCurrentPadding()` on `WAIT_OBJECT_0`, and wraps Win32 event handles in an RAII `EventHandleGuard`.
- **Seek accuracy on Android:** `exclusive_android.rs` adds `start_position_ms` to `AudioTrack.getPositionMs()` so seeking reports the true track offset instead of resetting to `0:00`.

## Transport & Queue Invariants

- **`pause()` from `Stopped`:** Remains `Stopped` (never enters `Paused` without an active track).
- **`previous_track()`:** Restarts the current track if playback position is `>= 3,000 ms`; otherwise steps to the previous queue item.
- **`restore_queue_order()` (disabling shuffle):** Preserves the already-played prefix without duplicating tracks in the remaining tail and discards stale pre-warmed decoders.
- **Remote tracks in queue:** `replace_queue` skips unavailable leading `remote://` tracks rather than attempting to decode an unresolved URI.
- **Shutdown safety:** `PlaybackService::shutdown()` is idempotent and safe whether invoked from inside or outside a Tokio runtime.
