import { useEffect, useRef, useState } from 'react';
import { HoverScrollText } from '../albums/HoverScrollText';
import { toLocalImageSrc } from '../../lib/runtime';
import type { PlaybackSnapshot, ScannedTrack } from '../../types/aria';
import { buildPlayerSubtitle, formatDuration } from '../library/view-models';

const SLEEP_TIMER_PRESETS_MINUTES = [15, 30, 45, 60, 90] as const;

type PlayerBarProps = {
  playback: PlaybackSnapshot;
  currentTrack: ScannedTrack | null;
  volume: number;
  onPrevious: () => void;
  onPlay: () => void;
  onPause: () => void;
  onNext: () => void;
  onSeek: (positionMs: number) => void | Promise<void>;
  onVolumeChange: (volume: number) => void | Promise<void>;
};

export function PlayerBar({
  playback,
  currentTrack,
  volume,
  onPrevious,
  onPlay,
  onPause,
  onNext,
  onSeek,
  onVolumeChange,
}: PlayerBarProps) {
  const progressRef = useRef<HTMLDivElement>(null);
  const sleepMenuRef = useRef<HTMLDivElement>(null);
  const onPauseRef = useRef(onPause);
  onPauseRef.current = onPause;

  const [scrubPositionMs, setScrubPositionMs] = useState<number | null>(null);
  const [isScrubbing, setIsScrubbing] = useState(false);
  const [previewPositionMs, setPreviewPositionMs] = useState<number | null>(null);
  const [sleepTimerEndsAt, setSleepTimerEndsAt] = useState<number | null>(null);
  const [sleepMenuOpen, setSleepMenuOpen] = useState(false);
  const [sleepRemainingMs, setSleepRemainingMs] = useState<number | null>(null);
  const [showArtPopup, setShowArtPopup] = useState(false);
  const normalizedVolume = clampUnitVolume(volume);
  const [draftVolumePercent, setDraftVolumePercent] = useState(() =>
    Math.round(normalizedVolume * 100),
  );
  const duration = playback.currentTrack?.durationMs ?? 0;
  const displayPositionMs =
    isScrubbing && scrubPositionMs !== null ? scrubPositionMs : playback.positionMs;
  const progress = duration > 0 ? Math.min(100, (displayPositionMs / duration) * 100) : 0;
  const previewMs =
    isScrubbing && scrubPositionMs !== null ? scrubPositionMs : previewPositionMs;
  const previewProgress =
    duration > 0 && previewMs !== null ? Math.min(100, (previewMs / duration) * 100) : 0;
  const art = toLocalImageSrc(currentTrack?.albumArtPath ?? null);
  const formatDetails = buildFormatDetails(currentTrack);
  const title = buildPlayerTitle(playback, currentTrack);
  const subtitle = buildPlayerCredits(playback, currentTrack);
  const isPlaying = playback.status === 'playing';
  const hasPrevious = (playback.currentQueueIndex ?? 0) > 0 || playback.positionMs > 0;
  const hasNext =
    playback.currentQueueIndex !== null &&
    playback.currentQueueIndex + 1 < playback.queue.length;
  const outputDeviceName = formatOutputDeviceName(playback.outputDevice.name);
  const outputMode = formatOutputMode(playback.outputDevice.backend);

  useEffect(() => {
    setScrubPositionMs(null);
    setIsScrubbing(false);
    setPreviewPositionMs(null);
    setShowArtPopup(false);
  }, [playback.currentTrack?.id]);

  useEffect(() => {
    if (!isScrubbing) {
      setScrubPositionMs(null);
    }
  }, [isScrubbing, playback.positionMs]);

  useEffect(() => {
    setDraftVolumePercent(Math.round(normalizedVolume * 100));
  }, [normalizedVolume]);

  useEffect(() => {
    if (sleepTimerEndsAt === null) {
      setSleepRemainingMs(null);
      return;
    }
    setSleepRemainingMs(Math.max(0, sleepTimerEndsAt - Date.now()));
    const id = setInterval(() => {
      const remaining = sleepTimerEndsAt - Date.now();
      if (remaining <= 0) {
        setSleepTimerEndsAt(null);
        setSleepRemainingMs(null);
        onPauseRef.current();
      } else {
        setSleepRemainingMs(remaining);
      }
    }, 1000);
    return () => clearInterval(id);
  }, [sleepTimerEndsAt]);

  useEffect(() => {
    if (!sleepMenuOpen) return;
    function handleClickOutside(event: MouseEvent) {
      if (sleepMenuRef.current && !sleepMenuRef.current.contains(event.target as Node)) {
        setSleepMenuOpen(false);
      }
    }
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [sleepMenuOpen]);

  function beginScrub() {
    if (duration <= 0) {
      return;
    }

    setIsScrubbing(true);
    setScrubPositionMs(displayPositionMs);
  }

  function updateScrub(nextPositionMs: number) {
    const clampedPosition = clampPosition(nextPositionMs, duration);
    setScrubPositionMs(clampedPosition);
    setPreviewPositionMs(clampedPosition);
  }

  function commitScrub(nextPositionMs?: number) {
    if (duration <= 0) {
      setIsScrubbing(false);
      setScrubPositionMs(null);
      return;
    }

    const resolvedPosition = clampPosition(
      nextPositionMs ?? scrubPositionMs ?? playback.positionMs,
      duration,
    );

    setIsScrubbing(false);
    setScrubPositionMs(null);

    if (Math.abs(resolvedPosition - playback.positionMs) >= 250) {
      void onSeek(resolvedPosition);
    }
  }

  function handlePreviewMove(clientX: number) {
    if (duration <= 0 || !progressRef.current) {
      return;
    }

    const bounds = progressRef.current.getBoundingClientRect();
    if (bounds.width <= 0) {
      return;
    }

    const ratio = Math.min(Math.max(0, (clientX - bounds.left) / bounds.width), 1);
    setPreviewPositionMs(Math.round(duration * ratio));
  }

  function handlePreviewLeave() {
    if (!isScrubbing) {
      setPreviewPositionMs(null);
    }
  }

  return (
    <section className="player-bar">
      <div className="player-bar__identity">
        {art ? (
          <img
            alt=""
            className="player-bar__art player-bar__art-image"
            onMouseEnter={() => setShowArtPopup(true)}
            onMouseLeave={() => setShowArtPopup(false)}
            src={art}
          />
        ) : (
          <div className="player-bar__art">
            <span>Aria</span>
          </div>
        )}
        <div className="player-bar__meta">
          <HoverScrollText className="player-bar__title" speed={42} text={title} />
          <HoverScrollText className="player-bar__subtitle" speed={34} text={subtitle} />
        </div>
      </div>

      <div className="player-bar__transport">
        <div className="player-bar__controls">
          <button
            aria-label="Previous track"
            className="ghost-button player-bar__control-button"
            disabled={!hasPrevious}
            onClick={onPrevious}
            type="button"
          >
            <PreviousIcon />
          </button>
          {isPlaying ? (
            <button
              aria-label="Pause"
              className="player-bar__control-button player-bar__control-button--primary"
              onClick={onPause}
              type="button"
            >
              <PauseIcon />
            </button>
          ) : (
            <button
              aria-label="Play"
              className="player-bar__control-button player-bar__control-button--primary"
              onClick={onPlay}
              type="button"
            >
              <PlayIcon />
            </button>
          )}
          <button
            aria-label="Next track"
            className="ghost-button player-bar__control-button"
            disabled={!hasNext}
            onClick={onNext}
            type="button"
          >
            <NextIcon />
          </button>
        </div>

        <div className="player-bar__timeline">
          <span>{formatDuration(displayPositionMs)}</span>
          <div
            className="player-progress"
            onPointerLeave={handlePreviewLeave}
            onPointerMove={(event) => handlePreviewMove(event.clientX)}
            ref={progressRef}
          >
            <div className="player-progress__fill" style={{ width: `${progress}%` }} />
            {previewMs !== null ? (
              <div
                className="player-progress__preview"
                style={{ left: `${previewProgress}%` }}
              >
                {formatDuration(previewMs)}
              </div>
            ) : null}
            <input
              aria-label="Seek playback position"
              className="player-progress__range"
              disabled={duration <= 0}
              max={duration || 1}
              min={0}
              onBlur={(event) => {
                if (isScrubbing) {
                  commitScrub(Number(event.currentTarget.value));
                }
              }}
              onChange={(event) => {
                updateScrub(Number(event.currentTarget.value));
              }}
              onKeyDown={() => beginScrub()}
              onKeyUp={(event) => {
                updateScrub(Number(event.currentTarget.value));
                commitScrub(Number(event.currentTarget.value));
              }}
              onPointerDown={(event) => {
                beginScrub();
                handlePreviewMove(event.clientX);
              }}
              onPointerUp={(event) => {
                updateScrub(Number(event.currentTarget.value));
                handlePreviewMove(event.clientX);
                commitScrub(Number(event.currentTarget.value));
              }}
              step={Math.max(100, Math.round(duration / 1000))}
              type="range"
              value={duration > 0 ? displayPositionMs : 0}
            />
          </div>
          <span>{formatDuration(duration)}</span>
        </div>
      </div>

      <div className="player-bar__facts">
        <label className="player-bar__volume" htmlFor="player-volume-range">
          <VolumeIcon muted={draftVolumePercent === 0} />
          <input
            aria-label="Playback volume"
            className="player-bar__volume-range"
            id="player-volume-range"
            max={100}
            min={0}
            onChange={(event) => {
              const nextVolumePercent = Number(event.currentTarget.value);
              setDraftVolumePercent(nextVolumePercent);
              void onVolumeChange(nextVolumePercent / 100);
            }}
            step={1}
            type="range"
            value={draftVolumePercent}
          />
          <span>{draftVolumePercent}%</span>
        </label>

        <div className="player-bar__device player-bar__device--source">
          <span>Source</span>
          <strong>{formatDetails}</strong>
        </div>

        <div className="player-bar__device player-bar__device--output">
          <span>Output</span>
          <strong title={playback.outputDevice.name}>{outputDeviceName}</strong>
          <small>{outputMode}</small>
        </div>

        <div className="player-bar__sleep-timer" ref={sleepMenuRef}>
          <button
            aria-expanded={sleepMenuOpen}
            aria-haspopup="true"
            aria-label={sleepTimerEndsAt !== null ? `Sleep timer active: ${formatSleepRemaining(sleepRemainingMs)} remaining` : 'Set sleep timer'}
            className={`ghost-button player-bar__sleep-button${sleepTimerEndsAt !== null ? ' player-bar__sleep-button--active' : ''}`}
            onClick={() => setSleepMenuOpen((prev) => !prev)}
            type="button"
          >
            <MoonIcon />
            {sleepRemainingMs !== null && (
              <span className="player-bar__sleep-remaining">{formatSleepRemaining(sleepRemainingMs)}</span>
            )}
          </button>
          {sleepMenuOpen && (
            <div className="player-bar__sleep-menu" role="menu">
              {SLEEP_TIMER_PRESETS_MINUTES.map((minutes) => (
                <button
                  className="player-bar__sleep-menu-item"
                  key={minutes}
                  onClick={() => {
                    setSleepTimerEndsAt(Date.now() + minutes * 60 * 1000);
                    setSleepMenuOpen(false);
                  }}
                  role="menuitem"
                  type="button"
                >
                  {minutes} min
                </button>
              ))}
              {sleepTimerEndsAt !== null && (
                <button
                  className="player-bar__sleep-menu-item player-bar__sleep-menu-item--cancel"
                  onClick={() => {
                    setSleepTimerEndsAt(null);
                    setSleepMenuOpen(false);
                  }}
                  role="menuitem"
                  type="button"
                >
                  Cancel timer
                </button>
              )}
            </div>
          )}
        </div>
      </div>

      {art && (
        <div
          className={`player-bar__art-popup${
            showArtPopup ? ' player-bar__art-popup--visible' : ''
          }`}
        >
          <img
            alt="Album art original size"
            className="player-bar__art-popup-image"
            src={art}
          />
        </div>
      )}
    </section>
  );
}

function buildFormatDetails(track: ScannedTrack | null): string {
  if (!track) {
    return 'No track loaded';
  }

  const details = [];

  if (track.audio.bitDepth) {
    details.push(`${track.audio.bitDepth}-bit`);
  }

  if (track.audio.sampleRate) {
    details.push(formatSampleRate(track.audio.sampleRate));
  }

  if (track.audio.format) {
    details.push(track.audio.format);
  }

  return details.join(' / ') || 'Unknown format';
}

function formatSampleRate(sampleRate: number): string {
  if (sampleRate >= 1000) {
    const kilohertz = sampleRate / 1000;
    return `${Number.isInteger(kilohertz) ? kilohertz : kilohertz.toFixed(1)} kHz`;
  }

  return `${sampleRate} Hz`;
}

function buildPlayerTitle(
  playback: PlaybackSnapshot,
  track: ScannedTrack | null,
): string {
  const trackTitle = firstMappedValue(track, 'title') ?? playback.currentTrack?.title ?? 'Nothing queued';
  const albumTitle = firstMappedValue(track, 'album');

  if (!albumTitle || sameDisplayValue(albumTitle, trackTitle)) {
    return trackTitle;
  }

  return `${albumTitle} - ${trackTitle}`;
}

function buildPlayerCredits(
  playback: PlaybackSnapshot,
  track: ScannedTrack | null,
): string {
  const credits = dedupeDisplayValues([
    ...mappedValues(track, 'conductor'),
    ...mappedValues(track, 'ensemble'),
    ...mappedValues(track, 'soloist'),
    ...mappedValues(track, 'composer'),
  ]);

  if (credits.length > 0) {
    return credits.join(' • ');
  }

  return buildPlayerSubtitle(playback);
}

function firstMappedValue(track: ScannedTrack | null, key: string): string | null {
  const value = track?.mappedFields[key]?.find((entry) => entry.trim().length > 0) ?? null;
  return value?.trim() ?? null;
}

function mappedValues(track: ScannedTrack | null, key: string): string[] {
  return (track?.mappedFields[key] ?? [])
    .map((value) => value.trim())
    .filter(Boolean);
}

function dedupeDisplayValues(values: string[]): string[] {
  const deduped: string[] = [];

  for (const value of values) {
    if (!deduped.some((entry) => sameDisplayValue(entry, value))) {
      deduped.push(value);
    }
  }

  return deduped;
}

function sameDisplayValue(left: string, right: string): boolean {
  return left.localeCompare(right, undefined, { sensitivity: 'accent' }) === 0;
}

function formatOutputDeviceName(name: string): string {
  const parentheticalSuffix = name.match(/^[^(]+?\((.+)\)\s*$/);
  if (parentheticalSuffix) {
    return parentheticalSuffix[1].trim();
  }

  return name;
}

function formatOutputMode(backend: string): string {
  return backend.replace(/^rodio\s*\/\s*/i, '').trim() || 'Output unavailable';
}

function clampPosition(positionMs: number, durationMs: number): number {
  return Math.min(Math.max(0, Math.round(positionMs)), durationMs);
}

function clampUnitVolume(volume: number): number {
  if (!Number.isFinite(volume)) {
    return 1;
  }

  return Math.min(Math.max(volume, 0), 1);
}

function formatSleepRemaining(ms: number | null): string {
  if (ms === null) return '';
  const totalSeconds = Math.ceil(ms / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
  }
  return `${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
}

function PlayIcon() {
  return (
    <svg
      aria-hidden="true"
      className="player-bar__control-icon player-bar__control-icon--play"
      viewBox="0 0 24 24"
    >
      <path d="M7.25 5.6c0-1.03 1.12-1.67 2.01-1.14l9.13 5.52a1.32 1.32 0 0 1 0 2.24l-9.13 5.52a1.32 1.32 0 0 1-2.01-1.14V5.6Z" />
    </svg>
  );
}

function PauseIcon() {
  return (
    <svg
      aria-hidden="true"
      className="player-bar__control-icon player-bar__control-icon--pause"
      viewBox="0 0 24 24"
    >
      <rect height="13.5" rx="1.35" width="4.2" x="5.6" y="5.25" />
      <rect height="13.5" rx="1.35" width="4.2" x="14.2" y="5.25" />
    </svg>
  );
}

function PreviousIcon() {
  return (
    <svg
      aria-hidden="true"
      className="player-bar__control-icon player-bar__control-icon--previous"
      viewBox="0 0 24 24"
    >
      <rect height="13" rx="1.1" width="2.8" x="4.75" y="5.5" />
      <path d="M17.95 6.08c.82-.54 1.93.05 1.93 1.04v9.76c0 .99-1.11 1.58-1.93 1.04l-6.93-4.88a1.25 1.25 0 0 1 0-2.08l6.93-4.88Z" />
    </svg>
  );
}

function NextIcon() {
  return (
    <svg
      aria-hidden="true"
      className="player-bar__control-icon player-bar__control-icon--next"
      viewBox="0 0 24 24"
    >
      <path d="M6.05 6.08c-.82-.54-1.93.05-1.93 1.04v9.76c0 .99 1.11 1.58 1.93 1.04l6.93-4.88a1.25 1.25 0 0 0 0-2.08L6.05 6.08Z" />
      <rect height="13" rx="1.1" width="2.8" x="16.45" y="5.5" />
    </svg>
  );
}

function VolumeIcon({ muted }: { muted: boolean }) {
  return (
    <svg
      aria-hidden="true"
      className="player-bar__volume-icon"
      viewBox="0 0 24 24"
    >
      <path d="M4.8 9.15a1 1 0 0 1 1-1h3.1l4.03-3.42a1 1 0 0 1 1.65.76v13.02a1 1 0 0 1-1.65.76L8.9 15.85H5.8a1 1 0 0 1-1-1V9.15Z" />
      {muted ? (
        <path d="M17.1 8.1a1 1 0 0 1 1.41 0L20 9.6l1.49-1.5a1 1 0 1 1 1.41 1.42L21.42 11l1.48 1.49a1 1 0 0 1-1.41 1.41L20 12.42l-1.49 1.48a1 1 0 0 1-1.41-1.41L18.58 11 17.1 9.51a1 1 0 0 1 0-1.41Z" />
      ) : (
        <path d="M17.42 8.05a1 1 0 0 1 1.4.12 5.38 5.38 0 0 1 0 7.66 1 1 0 1 1-1.52-1.3 3.38 3.38 0 0 0 0-5.06 1 1 0 0 1 .12-1.42Zm2.9-2.63a1 1 0 0 1 1.4.12 9.83 9.83 0 0 1 0 13.92 1 1 0 0 1-1.53-1.29 7.83 7.83 0 0 0 0-11.34 1 1 0 0 1 .13-1.41Z" />
      )}
    </svg>
  );
}

function MoonIcon() {
  return (
    <svg
      aria-hidden="true"
      className="player-bar__sleep-icon"
      viewBox="0 0 24 24"
    >
      <path d="M12.1 3a9 9 0 1 0 9 9 7 7 0 0 1-9-9Z" />
    </svg>
  );
}
