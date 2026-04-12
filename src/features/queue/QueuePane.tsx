import { SectionCard } from '../../components/SectionCard';
import type { PlaybackSnapshot, ScannedTrack } from '../../types/aria';
import { firstField, formatDuration, joinField } from '../library/view-models';

type QueuePaneProps = {
  playback: PlaybackSnapshot;
  tracks: ScannedTrack[];
  onClearQueue: () => void | Promise<void>;
  onRestoreOrder: () => void | Promise<void>;
  onShuffle: () => void | Promise<void>;
};

export function QueuePane({
  playback,
  tracks,
  onClearQueue,
  onRestoreOrder,
  onShuffle,
}: QueuePaneProps) {
  const trackLookup = new Map(tracks.map((track) => [track.id, track]));

  return (
    <div className="pane-stack">
      <SectionCard hideHeader>
        <div className="queue-pane__header">
          <div>
            <h2>Queue</h2>
          </div>
          <div className="inline-actions">
            <button
              className="ghost-button"
              onClick={() => void onRestoreOrder()}
              type="button"
            >
              Ordered
            </button>
            <button
              className="ghost-button"
              onClick={() => void onShuffle()}
              type="button"
            >
              Shuffle Remaining
            </button>
            <button
              className="ghost-button"
              disabled={playback.queue.length === 0}
              onClick={() => void onClearQueue()}
              type="button"
            >
              Clear queue
            </button>
          </div>
        </div>

        <ol className="queue-list">
          {playback.queue.length === 0 ? (
            <li className="empty-state">No queued tracks yet.</li>
          ) : (
            playback.queue.map((track, index) => (
              <QueueRow
                index={index}
                key={track.id}
                playback={playback}
                queuedTrack={track}
                scannedTrack={trackLookup.get(track.id) ?? null}
              />
            ))
          )}
        </ol>
      </SectionCard>
    </div>
  );
}

type QueueRowProps = {
  queuedTrack: PlaybackSnapshot['queue'][number];
  scannedTrack: ScannedTrack | null;
  playback: PlaybackSnapshot;
  index: number;
};

function QueueRow({ queuedTrack, scannedTrack, playback, index }: QueueRowProps) {
  const summary = buildQueueSummary(queuedTrack, scannedTrack);

  return (
    <li
      className={[
        'queue-list__item',
        playback.currentTrack?.id === queuedTrack.id ? 'queue-list__item--current' : '',
        playback.currentQueueIndex !== null && index < playback.currentQueueIndex
          ? 'queue-list__item--played'
          : '',
      ]
        .filter(Boolean)
        .join(' ')}
    >
      <div className="queue-list__summary">
        <strong className="queue-list__title">{summary.title}</strong>
        {summary.detail ? <span className="queue-list__meta"> / {summary.detail}</span> : null}
      </div>
      <span className="queue-list__duration">{formatDuration(queuedTrack.durationMs)}</span>
    </li>
  );
}

function buildQueueSummary(
  queuedTrack: PlaybackSnapshot['queue'][number],
  scannedTrack: ScannedTrack | null,
) {
  if (!scannedTrack) {
    return {
      title: queuedTrack.title,
      detail: queuedTrack.subtitle || '',
    };
  }

  const title = firstField(scannedTrack, 'title') || queuedTrack.title;
  const composer = joinField(scannedTrack, 'composer');
  const primaryCredit =
    joinField(scannedTrack, 'conductor') ||
    joinField(scannedTrack, 'soloist') ||
    joinField(scannedTrack, 'ensemble');

  return {
    title,
    detail: [composer, primaryCredit].filter(Boolean).join(' / '),
  };
}
