import type {
  MouseEvent as ReactMouseEvent,
  PointerEvent as ReactPointerEvent,
} from 'react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { SectionCard } from '../../components/SectionCard';
import { toLocalImageSrc } from '../../lib/runtime';
import type {
  LibraryFieldMapping,
  ScannedTrack,
  TrackTableSettings,
} from '../../types/aria';
import {
  albumIdForTrack,
  albumTitleForTrack,
  buildAlbumCards,
  buildTrackColumns,
  firstField,
  getTrackColumnValue,
} from '../library/view-models';
import { HoverScrollText } from './HoverScrollText';

type AlbumPaneProps = {
  tracks: ScannedTrack[];
  mappings: LibraryFieldMapping[];
  settings: TrackTableSettings;
  selectedAlbumId: string | null;
  onTrackTableChange: (settings: TrackTableSettings) => void;
  onPlayTracks: (tracks: ScannedTrack[]) => void | Promise<void>;
  onAddTracksToQueue: (tracks: ScannedTrack[]) => void | Promise<void>;
  onAddTracksToPlaylist: (tracks: ScannedTrack[]) => void | Promise<void>;
  onShowInExplorer: (track: ScannedTrack) => void | Promise<void>;
  onAddAlbumToPlaylist: (albumId: string) => void | Promise<void>;
  onAddToQueue: (albumId: string) => void | Promise<void>;
  onReplaceQueue: (albumId: string) => void | Promise<void>;
  onPlayAlbum: (albumId: string) => void | Promise<void>;
  onGoToDirectory: (albumId: string) => void | Promise<void>;
};

type DropPosition = 'before' | 'after';

type DropIndicator = {
  key: string;
  position: DropPosition;
};

type AlbumContextMenuState = {
  x: number;
  y: number;
};

type TrackContextMenuState = {
  x: number;
  y: number;
  trackIds: string[];
  primaryTrackId: string;
};

const defaultAlbumColumns = [
  'track_number',
  'title',
  'composer',
  'conductor',
  'ensemble',
  'soloist',
  'format',
  'duration',
];

export function AlbumPane({
  tracks,
  mappings,
  settings,
  selectedAlbumId,
  onTrackTableChange,
  onPlayTracks,
  onAddTracksToQueue,
  onAddTracksToPlaylist,
  onShowInExplorer,
  onAddAlbumToPlaylist,
  onAddToQueue,
  onReplaceQueue,
  onPlayAlbum,
  onGoToDirectory,
}: AlbumPaneProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const visibleDropIndicatorRef = useRef<DropIndicator | null>(null);
  const [selectedTrackIds, setSelectedTrackIds] = useState<string[]>([]);
  const [selectionAnchorId, setSelectionAnchorId] = useState<string | null>(null);
  const [isLayoutDialogOpen, setIsLayoutDialogOpen] = useState(false);
  const [visibleColumns, setVisibleColumns] = useState<string[]>([]);
  const [draggedVisibleColumn, setDraggedVisibleColumn] = useState<string | null>(
    null,
  );
  const [visibleDropIndicator, setVisibleDropIndicator] =
    useState<DropIndicator | null>(null);
  const [albumContextMenu, setAlbumContextMenu] =
    useState<AlbumContextMenuState | null>(null);
  const [trackContextMenu, setTrackContextMenu] =
    useState<TrackContextMenuState | null>(null);
  const [hasHydrated, setHasHydrated] = useState(false);
  const albums = useMemo(() => buildAlbumCards(tracks), [tracks]);
  const columns = useMemo(() => buildTrackColumns(mappings), [mappings]);
  const columnLookup = useMemo(
    () => new Map(columns.map((column) => [column.key, column])),
    [columns],
  );
  const selectedAlbum = useMemo(
    () => albums.find((album) => album.id === selectedAlbumId) ?? null,
    [albums, selectedAlbumId],
  );
  const albumTracks = useMemo(
    () =>
      tracks
        .filter(
          (track) =>
            selectedAlbumId !== null && albumIdForTrack(track) === selectedAlbumId,
        )
        .sort((left, right) => compareAlbumTracks(left, right)),
    [selectedAlbumId, tracks],
  );
  const normalizedVisibleColumns = useMemo(
    () => normalizeVisibleColumns(settings, columns),
    [columns, settings],
  );
  const selectedTrackSet = useMemo(
    () => new Set(selectedTrackIds),
    [selectedTrackIds],
  );

  useEffect(() => {
    setVisibleColumns(normalizedVisibleColumns);
    setHasHydrated(true);
  }, [normalizedVisibleColumns]);

  useEffect(() => {
    setSelectedTrackIds([]);
    setSelectionAnchorId(null);
  }, [selectedAlbumId]);

  useEffect(() => {
    const availableIds = new Set(albumTracks.map((track) => track.id));

    setSelectedTrackIds((current) => {
      const next = current.filter((trackId) => availableIds.has(trackId));
      return stringListsEqual(current, next) ? current : next;
    });

    setSelectionAnchorId((current) =>
      current && availableIds.has(current) ? current : null,
    );
  }, [albumTracks]);

  useEffect(() => {
    if (!draggedVisibleColumn) {
      return;
    }

    const handlePointerMove = (event: PointerEvent) => {
      const element = document
        .elementFromPoint(event.clientX, event.clientY)
        ?.closest<HTMLElement>('[data-reorder-key]');

      if (!element) {
        visibleDropIndicatorRef.current = null;
        setVisibleDropIndicator(null);
        return;
      }

      const key = element.dataset.reorderKey;
      if (!key || key === draggedVisibleColumn) {
        visibleDropIndicatorRef.current = null;
        setVisibleDropIndicator(null);
        return;
      }

      const bounds = element.getBoundingClientRect();
      const nextIndicator: DropIndicator = {
        key,
        position:
          event.clientY - bounds.top < bounds.height / 2 ? 'before' : 'after',
      };

      visibleDropIndicatorRef.current = nextIndicator;
      setVisibleDropIndicator(nextIndicator);
    };

    const handlePointerUp = () => {
      const nextIndicator = visibleDropIndicatorRef.current;

      if (nextIndicator) {
        setVisibleColumns((current) =>
          moveListItem(
            current,
            draggedVisibleColumn,
            nextIndicator.key,
            nextIndicator.position,
          ),
        );
      }

      visibleDropIndicatorRef.current = null;
      setDraggedVisibleColumn(null);
      setVisibleDropIndicator(null);
    };

    window.addEventListener('pointermove', handlePointerMove);
    window.addEventListener('pointerup', handlePointerUp);
    document.body.style.cursor = 'grabbing';
    document.body.style.userSelect = 'none';

    return () => {
      window.removeEventListener('pointermove', handlePointerMove);
      window.removeEventListener('pointerup', handlePointerUp);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    };
  }, [draggedVisibleColumn]);

  useEffect(() => {
    if (!albumContextMenu && !trackContextMenu) {
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

      setAlbumContextMenu(null);
      setTrackContextMenu(null);
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
  }, [albumContextMenu, trackContextMenu]);

  useEffect(() => {
    if (!isLayoutDialogOpen) {
      return undefined;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setIsLayoutDialogOpen(false);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isLayoutDialogOpen]);

  useEffect(() => {
    if (!hasHydrated) {
      return undefined;
    }

    if (visibleColumnsEqual(visibleColumns, normalizedVisibleColumns)) {
      return undefined;
    }

    const timeout = window.setTimeout(() => {
      onTrackTableChange({
        ...settings,
        visibleColumns,
      });
    }, 200);

    return () => window.clearTimeout(timeout);
  }, [
    hasHydrated,
    normalizedVisibleColumns,
    onTrackTableChange,
    settings,
    visibleColumns,
  ]);

  if (!selectedAlbum) {
    return (
      <div className="pane-stack">
        <SectionCard eyebrow="Album" title="Album detail">
          <div className="placeholder-pane">
            <strong>No album selected</strong>
            <p>Select an album from the Library tab to view the tracks inside it.</p>
          </div>
        </SectionCard>
      </div>
    );
  }

  const art = toLocalImageSrc(selectedAlbum.artPath);
  const selectedAlbumKey = selectedAlbum.id;

  function handleTrackClick(
    event: ReactMouseEvent<HTMLTableRowElement>,
    trackId: string,
  ) {
    const orderedIds = albumTracks.map((track) => track.id);

    if (event.shiftKey && selectionAnchorId) {
      const anchorIndex = orderedIds.indexOf(selectionAnchorId);
      const targetIndex = orderedIds.indexOf(trackId);

      if (anchorIndex !== -1 && targetIndex !== -1) {
        const [start, end] =
          anchorIndex < targetIndex
            ? [anchorIndex, targetIndex]
            : [targetIndex, anchorIndex];
        setSelectedTrackIds(orderedIds.slice(start, end + 1));
        return;
      }
    }

    if (event.ctrlKey || event.metaKey) {
      setSelectedTrackIds((current) =>
        current.includes(trackId)
          ? current.filter((id) => id !== trackId)
          : [...current, trackId],
      );
      setSelectionAnchorId(trackId);
      return;
    }

    setSelectedTrackIds((current) =>
      current.length > 1 && current.includes(trackId) ? current : [trackId],
    );
    setSelectionAnchorId(trackId);
  }

  function handleTrackDoubleClick(track: ScannedTrack) {
    const selectedTracks = selectedTrackSet.has(track.id)
      ? albumTracks.filter((candidate) => selectedTrackSet.has(candidate.id))
      : [track];

    void onPlayTracks(selectedTracks);
  }

  function handleTrackContextMenu(
    event: ReactMouseEvent<HTMLTableRowElement>,
    track: ScannedTrack,
  ) {
    event.preventDefault();
    event.stopPropagation();

    const nextSelectedTrackIds = selectedTrackSet.has(track.id)
      ? selectTrackIdsInOrder(albumTracks, selectedTrackIds)
      : [track.id];

    if (!selectedTrackSet.has(track.id)) {
      setSelectedTrackIds(nextSelectedTrackIds);
      setSelectionAnchorId(track.id);
    }

    setAlbumContextMenu(null);
    setTrackContextMenu({
      x: Math.min(event.clientX, window.innerWidth - 240),
      y: Math.min(event.clientY, window.innerHeight - 180),
      trackIds: nextSelectedTrackIds,
      primaryTrackId: track.id,
    });
  }

  function handleVisibleColumnDragStart(
    event: ReactPointerEvent<HTMLElement>,
    key: string,
  ) {
    event.preventDefault();
    event.stopPropagation();
    visibleDropIndicatorRef.current = null;
    setDraggedVisibleColumn(key);
    setVisibleDropIndicator(null);
  }

  function toggleColumn(key: string) {
    setVisibleColumns((current) => {
      if (current.includes(key)) {
        return current.length > 1
          ? current.filter((columnKey) => columnKey !== key)
          : current;
      }

      return [...current, key];
    });
  }

  function openContextMenu(event: ReactMouseEvent<HTMLDivElement>) {
    event.preventDefault();
    setTrackContextMenu(null);
    setAlbumContextMenu({
      x: Math.min(event.clientX, window.innerWidth - 240),
      y: Math.min(event.clientY, window.innerHeight - 220),
    });
  }

  async function runContextAction(
    action: (albumId: string) => void | Promise<void>,
  ) {
    setAlbumContextMenu(null);
    await action(selectedAlbumKey);
  }

  async function runTrackContextAction(
    action: (tracks: ScannedTrack[]) => void | Promise<void>,
  ) {
    if (!trackContextMenu) {
      return;
    }

    const selectedTracks = selectTracksInOrder(albumTracks, trackContextMenu.trackIds);
    if (selectedTracks.length === 0) {
      setTrackContextMenu(null);
      return;
    }

    setTrackContextMenu(null);
    await action(selectedTracks);
  }

  async function runShowInExplorer() {
    if (!trackContextMenu || trackContextMenu.trackIds.length !== 1) {
      return;
    }

    const track = albumTracks.find(
      (candidate) => candidate.id === trackContextMenu.primaryTrackId,
    );

    if (!track) {
      setTrackContextMenu(null);
      return;
    }

    setTrackContextMenu(null);
    await onShowInExplorer(track);
  }

  return (
    <div className="pane-stack">
      <SectionCard hideHeader>
        <div className="album-detail" onContextMenu={openContextMenu}>
          <div className="album-detail__toolbar">
            <p className="section-card__eyebrow">Album</p>
          </div>
          <div className="album-detail__hero">
            <div className="album-detail__art-column">
              {art ? (
                <img alt="" className="album-detail__art" src={art} />
              ) : (
                <div className="album-detail__art album-card__art--empty">Aria</div>
              )}
              <div className="album-detail__art-caption">
                {selectedAlbum.trackCount} tracks
              </div>
            </div>

            <div className="album-detail__meta">
              <HoverScrollText
                className="album-detail__title"
                speed={52}
                text={selectedAlbum.title}
              />
              {selectedAlbum.composer ? (
                <HoverScrollText
                  className="album-card__detail"
                  text={selectedAlbum.composer}
                />
              ) : null}
              {selectedAlbum.credit ? (
                <HoverScrollText
                  className="album-card__detail"
                  text={selectedAlbum.credit}
                />
              ) : null}
              {selectedAlbum.year ? (
                <div className="album-detail__facts">
                  <span>{selectedAlbum.year}</span>
                </div>
              ) : null}
            </div>

            <div className="album-detail__actions">
              <button
                className="ghost-button album-detail__layout-button"
                onClick={() => setIsLayoutDialogOpen(true)}
                type="button"
              >
                Layout
              </button>
            </div>
          </div>
        </div>

        <div className="track-table-shell">
          <table className="track-table album-track-table">
            <colgroup>
              {visibleColumns.map((key) => (
                <col
                  key={key}
                  style={{
                    width: `${settings.columnWidths[key] ?? getDefaultColumnWidth(key)}px`,
                  }}
                />
              ))}
            </colgroup>
            <thead>
              <tr>
                {visibleColumns.map((key) => (
                  <th key={key}>{columnLookup.get(key)?.label ?? key}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {albumTracks.length === 0 ? (
                <tr>
                  <td className="empty-state" colSpan={Math.max(visibleColumns.length, 1)}>
                    No tracks were found for this album.
                  </td>
                </tr>
              ) : (
                albumTracks.map((track) => (
                  <tr
                    aria-selected={selectedTrackSet.has(track.id)}
                    className={[
                      'track-table__row--playable',
                      selectedTrackSet.has(track.id) ? 'track-table__row--selected' : '',
                    ]
                      .filter(Boolean)
                      .join(' ')}
                    key={track.id}
                    onClick={(event) => handleTrackClick(event, track.id)}
                    onContextMenu={(event) => handleTrackContextMenu(event, track)}
                    onDoubleClick={() => handleTrackDoubleClick(track)}
                  >
                    {visibleColumns.map((key) => (
                      <td
                        className={key === 'path' ? 'track-path' : undefined}
                        key={`${track.id}-${key}`}
                      >
                        {albumTrackValue(track, key) || '-'}
                      </td>
                    ))}
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>

        {albumContextMenu ? (
          <div
            className="album-context-menu"
            ref={menuRef}
            style={{ left: `${albumContextMenu.x}px`, top: `${albumContextMenu.y}px` }}
          >
            <button onClick={() => void runContextAction(onAddAlbumToPlaylist)} type="button">
              Add to playlist
            </button>
            <button onClick={() => void runContextAction(onAddToQueue)} type="button">
              Add to queue
            </button>
            <button onClick={() => void runContextAction(onReplaceQueue)} type="button">
              Put in queue
            </button>
            <button onClick={() => void runContextAction(onPlayAlbum)} type="button">
              Play album
            </button>
            <button onClick={() => void runContextAction(onGoToDirectory)} type="button">
              Go to directory
            </button>
          </div>
        ) : null}

        {trackContextMenu ? (
          <div
            className="album-context-menu"
            ref={menuRef}
            style={{ left: `${trackContextMenu.x}px`, top: `${trackContextMenu.y}px` }}
          >
            <button
              onClick={() => void runTrackContextAction(onAddTracksToPlaylist)}
              type="button"
            >
              Add to playlist
            </button>
            <button
              onClick={() => void runTrackContextAction(onAddTracksToQueue)}
              type="button"
            >
              Add to queue
            </button>
            <button onClick={() => void runTrackContextAction(onPlayTracks)} type="button">
              Play
            </button>
            {trackContextMenu.trackIds.length === 1 ? (
              <button onClick={() => void runShowInExplorer()} type="button">
                Show in Explorer
              </button>
            ) : null}
          </div>
        ) : null}
      </SectionCard>

      {isLayoutDialogOpen ? (
        <div
          className="dialog-backdrop"
          onClick={() => setIsLayoutDialogOpen(false)}
          role="presentation"
        >
          <div
            className="dialog-card"
            onClick={(event) => event.stopPropagation()}
            role="dialog"
            aria-modal="true"
            aria-labelledby="album-layout-dialog-title"
          >
            <div className="dialog-card__header">
              <div>
                <p className="section-card__eyebrow">Album</p>
                <h3 id="album-layout-dialog-title">Track columns</h3>
              </div>
              <button
                className="ghost-button"
                onClick={() => setIsLayoutDialogOpen(false)}
                type="button"
              >
                Close
              </button>
            </div>

            <p className="dialog-card__copy">
              Choose which columns are visible for album tracks and drag the
              visible-column list to change the table order.
            </p>

            <div className="dialog-sections">
              <section className="dialog-section">
                <div className="dialog-section__header">
                  <h4>Visible columns</h4>
                  <span className="pane-chip">{visibleColumns.length} shown</span>
                </div>
                <ul className="reorder-list">
                  {visibleColumns.map((key) => {
                    const column = columnLookup.get(key);
                    const isDropBefore =
                      visibleDropIndicator?.key === key &&
                      visibleDropIndicator.position === 'before';
                    const isDropAfter =
                      visibleDropIndicator?.key === key &&
                      visibleDropIndicator.position === 'after';

                    return (
                      <li
                        className={[
                          'reorder-list__item',
                          draggedVisibleColumn === key
                            ? 'reorder-list__item--dragging'
                            : '',
                          isDropBefore ? 'reorder-list__item--drop-before' : '',
                          isDropAfter ? 'reorder-list__item--drop-after' : '',
                        ]
                          .filter(Boolean)
                          .join(' ')}
                        data-reorder-key={key}
                        key={key}
                      >
                        <div className="reorder-list__row">
                          <span
                            className="reorder-list__handle"
                            onPointerDown={(event) =>
                              handleVisibleColumnDragStart(event, key)
                            }
                            role="presentation"
                          >
                            Drag
                          </span>
                          <strong>{column?.label ?? key}</strong>
                          <span className="reorder-list__meta">
                            {settings.columnWidths[key] ?? getDefaultColumnWidth(key)} px
                          </span>
                          <button
                            className="ghost-button"
                            disabled={visibleColumns.length === 1}
                            onClick={() => toggleColumn(key)}
                            type="button"
                          >
                            Hide
                          </button>
                        </div>
                      </li>
                    );
                  })}
                </ul>
              </section>

              <section className="dialog-section">
                <div className="dialog-section__header">
                  <h4>Available columns</h4>
                </div>
                <div className="column-picker column-picker--dialog">
                  {columns.map((column) => (
                    <label className="column-toggle" key={column.key}>
                      <input
                        checked={visibleColumns.includes(column.key)}
                        onChange={() => toggleColumn(column.key)}
                        type="checkbox"
                      />
                      <span>{column.label}</span>
                    </label>
                  ))}
                </div>
              </section>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function compareAlbumTracks(left: ScannedTrack, right: ScannedTrack): number {
  const discDiff =
    parseSortNumber(firstField(left, 'disk_number')) -
    parseSortNumber(firstField(right, 'disk_number'));
  if (discDiff !== 0) {
    return discDiff;
  }

  const trackDiff =
    parseSortNumber(firstField(left, 'track_number')) -
    parseSortNumber(firstField(right, 'track_number'));
  if (trackDiff !== 0) {
    return trackDiff;
  }

  return (firstField(left, 'title') || left.fileName).localeCompare(
    firstField(right, 'title') || right.fileName,
    undefined,
    {
      numeric: true,
      sensitivity: 'base',
    },
  );
}

function parseSortNumber(value: string): number {
  const matched = value.match(/\d+/);
  if (!matched) {
    return Number.MAX_SAFE_INTEGER;
  }

  const parsed = Number.parseInt(matched[0], 10);
  return Number.isNaN(parsed) ? Number.MAX_SAFE_INTEGER : parsed;
}

function trackPosition(track: ScannedTrack): string {
  const disc = firstField(track, 'disk_number');
  const number = firstField(track, 'track_number');

  if (disc && number) {
    return `${disc}-${number}`;
  }

  return number || disc;
}

function albumTrackValue(track: ScannedTrack, key: string): string {
  if (key === 'track_number') {
    return trackPosition(track);
  }

  const value = getTrackColumnValue(track, key);
  if (key === 'title') {
    return value || albumTitleForTrack(track);
  }

  return value;
}

function normalizeVisibleColumns(
  settings: TrackTableSettings,
  columns: { key: string }[],
): string[] {
  const availableKeys = columns.map((column) => column.key);
  const configuredColumns = Array.from(
    new Set(settings.visibleColumns.filter((key) => availableKeys.includes(key))),
  );

  if (configuredColumns.length > 0) {
    return configuredColumns;
  }

  return defaultAlbumColumns.filter((key) => availableKeys.includes(key));
}

function selectTrackIdsInOrder(
  orderedTracks: ScannedTrack[],
  selectedTrackIds: string[],
): string[] {
  const selectedTrackSet = new Set(selectedTrackIds);
  return orderedTracks
    .filter((track) => selectedTrackSet.has(track.id))
    .map((track) => track.id);
}

function selectTracksInOrder(
  orderedTracks: ScannedTrack[],
  selectedTrackIds: string[],
): ScannedTrack[] {
  const selectedTrackSet = new Set(selectedTrackIds);
  return orderedTracks.filter((track) => selectedTrackSet.has(track.id));
}

function moveListItem(
  current: string[],
  sourceKey: string,
  targetKey: string,
  position: DropPosition,
): string[] {
  const withoutSource = current.filter((key) => key !== sourceKey);
  const targetIndex = withoutSource.indexOf(targetKey);

  if (targetIndex === -1) {
    return current;
  }

  const insertIndex = position === 'after' ? targetIndex + 1 : targetIndex;
  withoutSource.splice(insertIndex, 0, sourceKey);
  return withoutSource;
}

function visibleColumnsEqual(left: string[], right: string[]): boolean {
  if (left.length !== right.length) {
    return false;
  }

  for (let index = 0; index < left.length; index += 1) {
    if (left[index] !== right[index]) {
      return false;
    }
  }

  return true;
}

function stringListsEqual(left: string[], right: string[]): boolean {
  if (left.length !== right.length) {
    return false;
  }

  for (let index = 0; index < left.length; index += 1) {
    if (left[index] !== right[index]) {
      return false;
    }
  }

  return true;
}

function getDefaultColumnWidth(key: string): number {
  switch (key) {
    case 'track_number':
    case 'disk_number':
      return 96;
    case 'year':
    case 'format':
    case 'duration':
      return 110;
    case 'file_name':
      return 220;
    case 'path':
      return 360;
    case 'title':
      return 280;
    case 'album':
      return 260;
    case 'composer':
    case 'conductor':
    case 'ensemble':
    case 'soloist':
      return 220;
    default:
      return 180;
  }
}
