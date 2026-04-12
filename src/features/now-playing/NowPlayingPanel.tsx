import { SectionCard } from '../../components/SectionCard';
import type { PlaybackSnapshot } from '../../types/aria';

type NowPlayingPanelProps = {
  playback: PlaybackSnapshot;
  onPlay: () => void;
  onPause: () => void;
};

function formatDuration(durationMs: number) {
  const totalSeconds = Math.floor(durationMs / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, '0')}`;
}

export function NowPlayingPanel({
  playback,
  onPlay,
  onPause,
}: NowPlayingPanelProps) {
  const track = playback.currentTrack;

  return (
    <SectionCard
      eyebrow="Playback"
      title="Now playing"
      actions={
        <div className="inline-actions">
          <button onClick={onPlay} type="button">
            Play
          </button>
          <button className="ghost-button" onClick={onPause} type="button">
            Pause
          </button>
        </div>
      }
    >
      {track ? (
        <div className="track-panel">
          <div className="track-artwork" aria-hidden="true">
            <span>Aria</span>
          </div>
          <div className="track-meta">
            <h3>{track.title}</h3>
            <p>{track.subtitle}</p>
            <div className="track-stats">
              <span>Status: {playback.status}</span>
              <span>Queue depth: {playback.queueDepth}</span>
              <span>Length: {formatDuration(track.durationMs)}</span>
            </div>
          </div>
        </div>
      ) : (
        <p className="empty-state">No track is loaded yet.</p>
      )}

      <div className="device-chip">
        <strong>{playback.outputDevice.name}</strong>
        <span>
          {playback.outputDevice.backend}
          {playback.outputDevice.exclusiveCapable ? ' • exclusive capable' : ''}
        </span>
      </div>
    </SectionCard>
  );
}
