import { useEffect, useState } from 'react';
import { SectionCard } from '../../components/SectionCard';
import { TrackRawTagsDialog } from '../../components/TrackRawTagsDialog';
import { readTrackRawTags } from '../../lib/aria';
import { toLocalImageSrc } from '../../lib/runtime';
import type {
  LibraryFieldMapping,
  PlaybackSnapshot,
  ScannedTrack,
  TrackRawTags,
} from '../../types/aria';
import {
  albumIdForTrack,
  albumTitleForTrack,
  firstField,
  joinField,
} from '../library/view-models';
import {
  AlbumIcon,
  ArrowLeftIcon,
  EmptyArtPlaceholder,
} from '../../components/Icons';

type PlayerPaneProps = {
  playback: PlaybackSnapshot;
  currentTrack: ScannedTrack | null;
  mappings: LibraryFieldMapping[];
  isMobile?: boolean;
  onClose: () => void;
  onOpenAlbum?: (albumId: string) => void;
  onPlay?: () => void;
  onStop?: () => void;
  onPrevious?: () => void;
  onNext?: () => void;
  onSeek?: (positionMs: number) => void;
};

export function PlayerPane({
  playback,
  currentTrack,
  mappings: _mappings,
  isMobile: isMobileProp,
  onClose,
  onOpenAlbum,
}: PlayerPaneProps) {
  const isMobile =
    isMobileProp ??
    (typeof window !== 'undefined' &&
      (Boolean((window as any).AriaAndroid) ||
        /Android|iPhone|iPad|iPod|Mobile/i.test(navigator.userAgent)));

  const [rawTags, setRawTags] = useState<TrackRawTags | null>(null);
  const [isRawTagsOpen, setIsRawTagsOpen] = useState(false);
  const [isLoadingTags, setIsLoadingTags] = useState(false);

  const art = toLocalImageSrc(currentTrack?.albumArtPath ?? null);

  const work = currentTrack ? firstField(currentTrack, 'work') : '';
  const movement = currentTrack ? firstField(currentTrack, 'movement') : '';
  const title = currentTrack ? firstField(currentTrack, 'title') || currentTrack.fileName : playback.currentTrack?.title ?? 'No Track Loaded';
  const composer = currentTrack ? joinField(currentTrack, 'composer') : '';
  const conductor = currentTrack ? joinField(currentTrack, 'conductor') : '';
  const ensemble = currentTrack ? joinField(currentTrack, 'ensemble') : '';
  const soloist = currentTrack ? joinField(currentTrack, 'soloist') : '';
  const artist = currentTrack ? joinField(currentTrack, 'artist') : '';
  const albumTitle = currentTrack ? albumTitleForTrack(currentTrack) : '';
  const albumId = currentTrack ? albumIdForTrack(currentTrack) : null;
  const year = currentTrack ? firstField(currentTrack, 'year') || firstField(currentTrack, 'date') : '';
  const catalog = currentTrack ? firstField(currentTrack, 'catalog') || firstField(currentTrack, 'opus') : '';
  const genre = currentTrack ? joinField(currentTrack, 'genre') : '';
  const rawTrack = currentTrack ? firstField(currentTrack, 'track_number') : '';
  const rawDisk = currentTrack
    ? firstField(currentTrack, 'disk_number') || firstField(currentTrack, 'disc_number')
    : '';

  let trackDiskDisplay = '';
  if (rawTrack || rawDisk) {
    const cleanNumber = (val: string) => {
      const part = val.split('/')[0].trim();
      const parsed = parseInt(part, 10);
      return isNaN(parsed) ? part : String(parsed);
    };
    const trackNum = rawTrack ? cleanNumber(rawTrack) : '1';
    const diskNum = rawDisk ? cleanNumber(rawDisk) : '1';
    trackDiskDisplay = `${trackNum}/${diskNum}`;
  }

  const showArtist = Boolean(
    artist &&
    artist !== composer &&
    artist !== conductor &&
    artist !== ensemble &&
    artist !== soloist
  );

  const audio = currentTrack?.audio;
  const audioParts: string[] = [];
  if (audio) {
    let spec = audio.format.toUpperCase();
    if (audio.bitDepth && audio.sampleRate) {
      spec += ` ${audio.bitDepth}-bit / ${(audio.sampleRate / 1000).toFixed(1)} kHz`;
    } else if (audio.sampleRate) {
      spec += ` ${(audio.sampleRate / 1000).toFixed(1)} kHz`;
    } else if (audio.bitDepth) {
      spec += ` ${audio.bitDepth}-bit`;
    }
    audioParts.push(spec);
    if (audio.channels) {
      audioParts.push(audio.channels === 1 ? 'Mono' : audio.channels === 2 ? 'Stereo' : `${audio.channels} ch`);
    }
  }
  const audioSummary = audioParts.join(' • ');

  const deviceName = playback.outputDevice?.name;
  const deviceBackend = playback.outputDevice?.backend;
  const deviceSummary = deviceName
    ? `${deviceName}${deviceBackend ? ` (${deviceBackend})` : ''}`
    : null;

  const playbackSummary = [audioSummary, deviceSummary].filter(Boolean).join(' — ') || null;

  useEffect(() => {
    setRawTags(null);
    setIsRawTagsOpen(false);
  }, [currentTrack?.id]);

  async function handleOpenRawTags() {
    if (!currentTrack) return;
    if (rawTags) {
      setIsRawTagsOpen(true);
      return;
    }
    setIsLoadingTags(true);
    try {
      const tags = await readTrackRawTags(currentTrack.id);
      setRawTags(tags);
      setIsRawTagsOpen(true);
    } catch (err) {
      console.error('Failed to load track raw tags:', err);
    } finally {
      setIsLoadingTags(false);
    }
  }

  if (!currentTrack && !playback.currentTrack) {
    return (
      <div className="pane-stack player-pane">
        <SectionCard hideHeader>
          <div className="album-detail__toolbar">
            <button
              className="ghost-button album-detail__back-button"
              onClick={onClose}
              type="button"
            >
              <ArrowLeftIcon size={14} />
              <span>Back</span>
            </button>
            <p className="section-card__eyebrow">Now Playing</p>
          </div>
          <div className="placeholder-pane">
            <div className="placeholder-pane__art" style={{ width: '120px', height: '120px', margin: '0 auto 1.5rem' }}>
              <EmptyArtPlaceholder />
            </div>
            <strong>No track is currently playing</strong>
            <p>Select a track or album from your library to begin listening.</p>
            <button
              className="ghost-button"
              onClick={onClose}
              style={{ marginTop: '1rem', display: 'inline-flex', alignItems: 'center', gap: '0.45rem' }}
              type="button"
            >
              <ArrowLeftIcon size={14} />
              <span>Go to Library</span>
            </button>
          </div>
        </SectionCard>
      </div>
    );
  }

  return (
    <div className="pane-stack player-pane">
      <SectionCard hideHeader>
        <div className="player-pane__toolbar">
          <button
            className="ghost-button album-detail__back-button"
            onClick={onClose}
            title="Back to previous view"
            type="button"
          >
            <ArrowLeftIcon size={14} />
            <span>Back</span>
          </button>
          <div className="player-pane__toolbar-meta">
            <p className="section-card__eyebrow">Now Playing</p>
            {playback.queueDepth > 0 && playback.currentQueueIndex !== null && (
              <span className="player-pane__queue-badge">
                Track {playback.currentQueueIndex + 1} of {playback.queueDepth}
              </span>
            )}
          </div>
          {albumId && onOpenAlbum && (
            <button
              className="ghost-button player-pane__album-button"
              onClick={() => onOpenAlbum(albumId)}
              title="View full album details"
              type="button"
            >
              <AlbumIcon size={14} />
              <span>View Album</span>
            </button>
          )}
        </div>

        <div className="player-pane__hero">
          <div className="player-pane__art-col">
            <div className="player-pane__art-wrapper">
              {art ? (
                <img
                  alt={`${title} artwork`}
                  className="player-pane__art"
                  src={art}
                />
              ) : (
                <div className="player-pane__art player-pane__art--empty">
                  <EmptyArtPlaceholder />
                </div>
              )}
            </div>
          </div>

          <div className="player-pane__meta-col">
            {work && <h2 className="player-pane__work">{work}</h2>}
            <h1 className="player-pane__title">{movement || title}</h1>

            <div className="player-pane__info-list">
              {composer && (
                <div className="player-pane__info-row">
                  <span className="player-pane__info-label">Composer</span>
                  <span className="player-pane__info-value player-pane__info-value--highlight">
                    {composer}
                  </span>
                </div>
              )}
              {conductor && (
                <div className="player-pane__info-row">
                  <span className="player-pane__info-label">Conductor</span>
                  <span className="player-pane__info-value">{conductor}</span>
                </div>
              )}
              {ensemble && (
                <div className="player-pane__info-row">
                  <span className="player-pane__info-label">Ensemble</span>
                  <span className="player-pane__info-value">{ensemble}</span>
                </div>
              )}
              {soloist && (
                <div className="player-pane__info-row">
                  <span className="player-pane__info-label">Soloist</span>
                  <span className="player-pane__info-value">{soloist}</span>
                </div>
              )}
              {showArtist && (
                <div className="player-pane__info-row">
                  <span className="player-pane__info-label">Artist</span>
                  <span className="player-pane__info-value">{artist}</span>
                </div>
              )}
              {albumTitle && (
                <div className="player-pane__info-row">
                  <span className="player-pane__info-label">Album</span>
                  <span className="player-pane__info-value">{albumTitle}</span>
                </div>
              )}
              {year && (
                <div className="player-pane__info-row">
                  <span className="player-pane__info-label">Year / Date</span>
                  <span className="player-pane__info-value">{year}</span>
                </div>
              )}
              {catalog && (
                <div className="player-pane__info-row">
                  <span className="player-pane__info-label">Catalog / Op.</span>
                  <span className="player-pane__info-value">{catalog}</span>
                </div>
              )}
              {movement && title && title !== movement && (
                <div className="player-pane__info-row">
                  <span className="player-pane__info-label">Title</span>
                  <span className="player-pane__info-value">{title}</span>
                </div>
              )}
              {genre && (
                <div className="player-pane__info-row">
                  <span className="player-pane__info-label">Genre</span>
                  <span className="player-pane__info-value">{genre}</span>
                </div>
              )}
              {trackDiskDisplay && (
                <div className="player-pane__info-row">
                  <span className="player-pane__info-label">Track/Disk</span>
                  <span className="player-pane__info-value">{trackDiskDisplay}</span>
                </div>
              )}
              {playbackSummary && (
                <div className="player-pane__info-row">
                  <span className="player-pane__info-label">Playback</span>
                  <span
                    className="player-pane__info-value"
                    title="Audio format, sample rate, and output device"
                  >
                    {playbackSummary}
                  </span>
                </div>
              )}
              {!isMobile && currentTrack?.path && (
                <div className="player-pane__info-row">
                  <span className="player-pane__info-label">File</span>
                  <span className="player-pane__info-value player-pane__info-path">
                    {currentTrack.path}
                  </span>
                </div>
              )}
              {!isMobile && currentTrack && (
                <div className="player-pane__info-row">
                  <span className="player-pane__info-label">Raw Tags</span>
                  <span className="player-pane__info-value">
                    <button
                      className="ghost-button player-pane__raw-tags-link"
                      disabled={isLoadingTags}
                      onClick={() => void handleOpenRawTags()}
                      type="button"
                    >
                      <span>{isLoadingTags ? 'Loading Tags...' : 'Inspect Raw File Tags'}</span>
                    </button>
                  </span>
                </div>
              )}
            </div>
          </div>
        </div>
      </SectionCard>

      <SectionCard eyebrow="Notes & Context" title="Background Information">
        <div className="player-pane__notes-grid">
          <div className="player-pane__note-card">
            <div className="player-pane__note-header">
              <span className="player-pane__note-eyebrow">Composition</span>
              <h3 className="player-pane__note-title">{work || title}</h3>
            </div>
            <div className="player-pane__note-body">
              <p className="player-pane__note-placeholder">
                Historical context, program commentary, compositional structure, and movement analysis for this piece will appear here.
              </p>
              <div className="player-pane__note-footer">
                <span className="player-pane__note-hint">Program Notes & Analysis (Reserved)</span>
              </div>
            </div>
          </div>

          <div className="player-pane__note-card">
            <div className="player-pane__note-header">
              <span className="player-pane__note-eyebrow">Artist & Composer</span>
              <h3 className="player-pane__note-title">{composer || conductor || 'Artist'}</h3>
            </div>
            <div className="player-pane__note-body">
              <p className="player-pane__note-placeholder">
                Biographical details, historical period, composer chronology, and profiles for the orchestra, conductor, and soloists will appear here.
              </p>
              <div className="player-pane__note-footer">
                <span className="player-pane__note-hint">Biographical & Ensemble Profiles (Reserved)</span>
              </div>
            </div>
          </div>
        </div>
      </SectionCard>

      <TrackRawTagsDialog
        error={null}
        isLoading={isLoadingTags}
        isOpen={isRawTagsOpen}
        onClose={() => setIsRawTagsOpen(false)}
        tags={rawTags ?? {}}
        track={currentTrack}
      />
    </div>
  );
}
