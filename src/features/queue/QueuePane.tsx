import { useEffect, useRef, useState } from 'react';
import type { RefObject } from 'react';
import { SectionCard } from '../../components/SectionCard';
import { HoverScrollText } from '../albums/HoverScrollText';
import type { PlaybackSnapshot, ScannedTrack } from '../../types/aria';
import { albumIdForTrack, firstField, formatDuration, joinField } from '../library/view-models';

type QueuePaneProps = {
  playback: PlaybackSnapshot;
  tracks: ScannedTrack[];
  onClearQueue: () => void | Promise<void>;
  onRestoreOrder: () => void | Promise<void>;
  onShuffle: () => void | Promise<void>;
  onOpenAlbum: (albumId: string) => void;
  onOpenTrack: (trackId: string) => void;
};

type QueueContextMenuState = {
  trackId: string;
  albumId: string | null;
  x: number;
  y: number;
};

export function QueuePane({
  playback,
  tracks,
  onClearQueue,
  onRestoreOrder,
  onShuffle,
  onOpenAlbum,
  onOpenTrack,
}: QueuePaneProps) {
  const trackLookup = new Map(tracks.map((track) => [track.id, track]));
  const currentTrackRef = useRef<HTMLLIElement | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [contextMenu, setContextMenu] = useState<QueueContextMenuState | null>(null);

  useEffect(() => {
    if (!playback.currentTrack) {
      return;
    }

    currentTrackRef.current?.scrollIntoView({
      block: 'center',
      inline: 'nearest',
    });
  }, [playback.currentQueueIndex, playback.currentTrack?.id]);

  useEffect(() => {
    if (!contextMenu) {
      return undefined;
    }

    const closeMenu = (event?: Event) => {
      if (
        event instanceof MouseEvent &&
        menuRef.current?.contains(event.target as Node)
      ) {
        return;
      }

      if (event instanceof KeyboardEvent && event.key !== 'Escape') {
        return;
      }

      setContextMenu(null);
    };

    window.addEventListener('pointerdown', closeMenu);
    window.addEventListener('resize', closeMenu);
    window.addEventListener('keydown', closeMenu);
    window.addEventListener('scroll', closeMenu, true);

    return () => {
      window.removeEventListener('pointerdown', closeMenu);
      window.removeEventListener('resize', closeMenu);
      window.removeEventListener('keydown', closeMenu);
      window.removeEventListener('scroll', closeMenu, true);
    };
  }, [contextMenu]);

  return (
    <div className="pane-stack queue-pane">
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
                currentTrackRef={
                  playback.currentTrack?.id === track.id ? currentTrackRef : null
                }
                playback={playback}
                queuedTrack={track}
                scannedTrack={trackLookup.get(track.id) ?? null}
                onContextMenu={(event, trackId, albumId) => {
                  event.preventDefault();
                  setContextMenu({
                    trackId,
                    albumId,
                    x: Math.min(event.clientX, window.innerWidth - 240),
                    y: Math.min(event.clientY, window.innerHeight - 140),
                  });
                }}
              />
            ))
          )}
        </ol>

        {contextMenu ? (
          <div
            className="album-context-menu"
            ref={menuRef}
            style={{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }}
          >
            <button
              onClick={() => {
                onOpenTrack(contextMenu.trackId);
                setContextMenu(null);
              }}
              type="button"
            >
              Go to track
            </button>
            <button
              disabled={!contextMenu.albumId}
              onClick={() => {
                if (contextMenu.albumId) {
                  onOpenAlbum(contextMenu.albumId);
                }
                setContextMenu(null);
              }}
              type="button"
            >
              Go to album
            </button>
          </div>
        ) : null}
      </SectionCard>
    </div>
  );
}

type QueueRowProps = {
  queuedTrack: PlaybackSnapshot['queue'][number];
  scannedTrack: ScannedTrack | null;
  playback: PlaybackSnapshot;
  index: number;
  currentTrackRef: RefObject<HTMLLIElement> | null;
  onContextMenu: (event: React.MouseEvent, trackId: string, albumId: string | null) => void;
};

function QueueRow({
  queuedTrack,
  scannedTrack,
  playback,
  index,
  currentTrackRef,
  onContextMenu,
}: QueueRowProps) {
  const summary = buildQueueSummary(queuedTrack, scannedTrack);
  const fullSummary = summary.detail
    ? `${summary.title} / ${summary.detail}`
    : summary.title;
  const albumId = scannedTrack ? albumIdForTrack(scannedTrack) : null;

  return (
    <li
      ref={currentTrackRef}
      onContextMenu={(event) => onContextMenu(event, queuedTrack.id, albumId)}
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
        <HoverScrollText
          className="queue-list__summary-line"
          speed={34}
          text={fullSummary}
        >
          <>
            <strong className="queue-list__title">{summary.title}</strong>
            {summary.detail ? (
              <span className="queue-list__meta"> / {summary.detail}</span>
            ) : null}
          </>
        </HoverScrollText>
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
