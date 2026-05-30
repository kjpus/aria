import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
} from 'react';
import { SectionCard } from '../../components/SectionCard';
import { toLocalImageSrc } from '../../lib/runtime';
import type {
  Playlist,
  ScannedTrack,
  TrackTableSettings,
} from '../../types/aria';
import {
  albumIdForTrack,
  albumTitleForTrack,
  firstField,
  formatDuration,
  joinField,
  tracksForPlaylist,
} from '../library/view-models';
import { HoverScrollText } from '../albums/HoverScrollText';

type PlaylistColumnKey =
  | 'playlist_order'
  | 'album'
  | 'title'
  | 'composer'
  | 'artists'
  | 'duration';

type PlaylistColumn = {
  key: PlaylistColumnKey;
  label: string;
};

type PlaylistPaneProps = {
  playlists: Playlist[];
  tracks: ScannedTrack[];
  settings: TrackTableSettings;
  selectedPlaylistId: string | null;
  onSelectPlaylist: (playlistId: string) => void;
  onTrackTableChange: (settings: TrackTableSettings) => void;
  onRenamePlaylist: (playlistId: string, name: string) => void | Promise<void>;
  onDeletePlaylist: (playlistId: string) => void | Promise<void>;
  onRegeneratePlaylistIcon: (playlistId: string) => void | Promise<void>;
  onExportPlaylist: (playlistId: string) => void | Promise<void>;
  onAddPlaylistToQueue: (playlistId: string) => void | Promise<void>;
  onPlayPlaylist: (playlistId: string) => void | Promise<void>;
  onShufflePlayPlaylist: (playlistId: string) => void | Promise<void>;
  onPlayTracks: (tracks: ScannedTrack[]) => void | Promise<void>;
  onRemoveTracks: (playlistId: string, trackIds: string[]) => void | Promise<void>;
  onOpenAlbum: (albumId: string) => void;
};

type PlaylistContextMenuState = {
  playlistId: string;
  x: number;
  y: number;
};

type PlaylistTrackContextMenuState = {
  x: number;
  y: number;
  trackIds: string[];
  primaryTrackId: string;
};

type ResizeState = {
  key: PlaylistColumnKey;
  partnerKey: PlaylistColumnKey;
  startX: number;
  startWidth: number;
  startPartnerWidth: number;
};

export function PlaylistPane({
  playlists,
  tracks,
  settings,
  selectedPlaylistId,
  onSelectPlaylist,
  onTrackTableChange,
  onRenamePlaylist,
  onDeletePlaylist,
  onRegeneratePlaylistIcon,
  onExportPlaylist,
  onAddPlaylistToQueue,
  onPlayPlaylist,
  onShufflePlayPlaylist,
  onPlayTracks,
  onRemoveTracks,
}: PlaylistPaneProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const resizeStateRef = useRef<ResizeState | null>(null);
  const [contextMenu, setContextMenu] = useState<PlaylistContextMenuState | null>(null);
  const [trackContextMenu, setTrackContextMenu] =
    useState<PlaylistTrackContextMenuState | null>(null);
  const [renamePlaylistId, setRenamePlaylistId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const [selectedTrackIds, setSelectedTrackIds] = useState<string[]>([]);
  const [selectionAnchorId, setSelectionAnchorId] = useState<string | null>(null);
  const [columnWidths, setColumnWidths] = useState<Record<string, number>>({});
  const [resizingColumn, setResizingColumn] = useState<string | null>(null);
  const [hasHydrated, setHasHydrated] = useState(false);
  const columns = useMemo<PlaylistColumn[]>(
    () => [
      { key: 'playlist_order', label: '#' },
      { key: 'album', label: 'Album' },
      { key: 'title', label: 'Title' },
      { key: 'composer', label: 'Composer' },
      { key: 'artists', label: 'Artists' },
      { key: 'duration', label: 'Duration' },
    ],
    [],
  );
  const visibleColumns = useMemo<PlaylistColumnKey[]>(
    () => columns.map((column) => column.key),
    [columns],
  );
  const normalizedColumnWidths = useMemo(
    () => normalizeColumnWidths(settings, columns),
    [columns, settings],
  );
  const columnLookup = useMemo(
    () => new Map(columns.map((column) => [column.key, column])),
    [columns],
  );
  const selectedPlaylist =
    playlists.find((playlist) => playlist.id === selectedPlaylistId) ?? playlists[0] ?? null;
  const playlistTracks = useMemo(
    () => (selectedPlaylist ? tracksForPlaylist(tracks, selectedPlaylist) : []),
    [tracks, selectedPlaylist],
  );
  const playlistRows = useMemo(
    () => playlistTracks.map((track, index) => ({ track, order: index + 1 })),
    [playlistTracks],
  );
  const playlistTrackIdsKey = useMemo(
    () => playlistTracks.map((track) => track.id).join('\u0001'),
    [playlistTracks],
  );
  const selectedTrackSet = useMemo(
    () => new Set(selectedTrackIds),
    [selectedTrackIds],
  );
  const renameTarget =
    playlists.find((playlist) => playlist.id === renamePlaylistId) ?? null;

  useEffect(() => {
    setColumnWidths(normalizedColumnWidths);
    setHasHydrated(true);
  }, [normalizedColumnWidths]);

  useEffect(() => {
    if (!resizingColumn) {
      return undefined;
    }

    const handlePointerMove = (event: PointerEvent) => {
      const resizeState = resizeStateRef.current;
      if (!resizeState) {
        return;
      }

      const nextWidths = resizeAdjacentColumns(
        resizeState.key,
        resizeState.partnerKey,
        resizeState.startWidth,
        resizeState.startPartnerWidth,
        event.clientX - resizeState.startX,
      );

      setColumnWidths((current) => ({
        ...current,
        ...nextWidths,
      }));
    };

    const handlePointerUp = () => {
      resizeStateRef.current = null;
      setResizingColumn(null);
    };

    window.addEventListener('pointermove', handlePointerMove);
    window.addEventListener('pointerup', handlePointerUp);
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';

    return () => {
      window.removeEventListener('pointermove', handlePointerMove);
      window.removeEventListener('pointerup', handlePointerUp);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    };
  }, [resizingColumn]);

  useEffect(() => {
    if (!hasHydrated) {
      return undefined;
    }

    if (
      columnWidthsEqual(columnWidths, normalizedColumnWidths) &&
      stringListsEqual(settings.visibleColumns, visibleColumns)
    ) {
      return undefined;
    }

    const timeout = window.setTimeout(() => {
      onTrackTableChange({
        ...settings,
        visibleColumns,
        columnWidths,
      });
    }, 200);

    return () => window.clearTimeout(timeout);
  }, [
    columnWidths,
    hasHydrated,
    normalizedColumnWidths,
    onTrackTableChange,
    settings,
  ]);

  useEffect(() => {
    if (!contextMenu && !trackContextMenu) {
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
  }, [contextMenu, trackContextMenu]);

  useEffect(() => {
    if (!renameTarget) {
      return undefined;
    }

    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setRenamePlaylistId(null);
      }
    };

    window.addEventListener('keydown', closeOnEscape);
    return () => window.removeEventListener('keydown', closeOnEscape);
  }, [renameTarget]);

  useEffect(() => {
    setSelectedTrackIds((current) => {
      const available = new Set(playlistTracks.map((track) => track.id));
      return current.filter((trackId) => available.has(trackId));
    });
    setSelectionAnchorId((current) =>
      current && playlistTracks.some((track) => track.id === current) ? current : null,
    );
    setTrackContextMenu(null);
  }, [selectedPlaylist?.id, playlistTrackIdsKey]);

  function openContextMenu(
    event: ReactMouseEvent<HTMLButtonElement>,
    playlistId: string,
  ) {
    event.preventDefault();
    onSelectPlaylist(playlistId);
    setTrackContextMenu(null);
    setContextMenu({
      playlistId,
      x: Math.min(event.clientX, window.innerWidth - 240),
      y: Math.min(event.clientY, window.innerHeight - 220),
    });
  }

  async function runPlaylistAction(
    action: (playlistId: string) => void | Promise<void>,
  ) {
    if (!contextMenu) {
      return;
    }

    const { playlistId } = contextMenu;
    setContextMenu(null);
    await action(playlistId);
  }

  function openRenameDialog() {
    if (!contextMenu) {
      return;
    }

    const playlist = playlists.find((item) => item.id === contextMenu.playlistId);
    if (!playlist) {
      return;
    }

    setRenamePlaylistId(playlist.id);
    setRenameValue(playlist.name);
    setContextMenu(null);
  }

  async function handleDeletePlaylist() {
    if (!contextMenu) {
      return;
    }

    const playlist = playlists.find((item) => item.id === contextMenu.playlistId);
    if (!playlist) {
      return;
    }

    setContextMenu(null);
    if (window.confirm(`Delete playlist "${playlist.name}"?`)) {
      await onDeletePlaylist(playlist.id);
    }
  }

  function handleTrackClick(
    event: ReactMouseEvent<HTMLTableRowElement>,
    trackId: string,
  ) {
    const orderedIds = playlistTracks.map((track) => track.id);

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

  function handleTrackContextMenu(
    event: ReactMouseEvent<HTMLTableRowElement>,
    track: ScannedTrack,
  ) {
    event.preventDefault();
    event.stopPropagation();

    const nextSelectedTrackIds = selectedTrackSet.has(track.id)
      ? selectTrackIdsInOrder(playlistTracks, selectedTrackIds)
      : [track.id];

    if (!selectedTrackSet.has(track.id)) {
      setSelectedTrackIds(nextSelectedTrackIds);
      setSelectionAnchorId(track.id);
    }

    setContextMenu(null);
    setTrackContextMenu({
      x: Math.min(event.clientX, window.innerWidth - 240),
      y: Math.min(event.clientY, window.innerHeight - 140),
      trackIds: nextSelectedTrackIds,
      primaryTrackId: track.id,
    });
  }

  function handleResizeStart(
    event: ReactPointerEvent<HTMLDivElement>,
    key: PlaylistColumnKey,
    partnerKey: PlaylistColumnKey | null,
  ) {
    event.preventDefault();
    event.stopPropagation();

    if (!partnerKey) {
      return;
    }

    resizeStateRef.current = {
      key,
      partnerKey,
      startX: event.clientX,
      startWidth: columnWidths[key] ?? getDefaultColumnWidth(key),
      startPartnerWidth:
        columnWidths[partnerKey] ?? getDefaultColumnWidth(partnerKey),
    };
    setResizingColumn(key);
  }

  async function runTrackContextAction(
    action: (playlistId: string, trackIds: string[]) => void | Promise<void>,
  ) {
    if (!trackContextMenu || !selectedPlaylist) {
      return;
    }

    const nextTrackIds = selectTrackIdsInOrder(playlistTracks, trackContextMenu.trackIds);
    if (nextTrackIds.length === 0) {
      setTrackContextMenu(null);
      return;
    }

    await action(selectedPlaylist.id, nextTrackIds);
    setTrackContextMenu(null);
  }

  async function submitRenamePlaylist() {
    if (!renameTarget || !renameValue.trim()) {
      return;
    }

    await onRenamePlaylist(renameTarget.id, renameValue);
    setRenamePlaylistId(null);
  }

  return (
    <div className="pane-stack">
      <SectionCard hideHeader>
        {playlists.length === 0 ? (
          <div className="placeholder-pane">
            <strong>No playlists yet</strong>
            <p>Add albums or tracks to a playlist from Library, Album, or Tracks.</p>
          </div>
        ) : (
          <>
            <div className="playlist-carousel">
              {playlists.map((playlist) => {
                const playlistTracks = tracksForPlaylist(tracks, playlist);
                const collageArts = buildPlaylistCollage(playlist, playlistTracks);
                const selected = selectedPlaylist?.id === playlist.id;

                return (
                  <button
                    className={[
                      'playlist-card',
                      selected ? 'playlist-card--selected' : '',
                    ]
                      .filter(Boolean)
                      .join(' ')}
                    key={playlist.id}
                    onClick={() => onSelectPlaylist(playlist.id)}
                    onContextMenu={(event) => openContextMenu(event, playlist.id)}
                    onDoubleClick={() => void onPlayPlaylist(playlist.id)}
                    type="button"
                  >
                    <div className="playlist-card__collage">
                      {collageArts.map((artPath, index) => {
                        const src = toLocalImageSrc(artPath);
                        return src ? (
                          <img
                            alt=""
                            className="playlist-card__tile"
                            key={`${playlist.id}-art-${index}`}
                            src={src}
                          />
                        ) : (
                          <div
                            className="playlist-card__tile playlist-card__tile--empty"
                            key={`${playlist.id}-art-${index}`}
                          >
                            Aria
                          </div>
                        );
                      })}
                    </div>
                    <div className="playlist-card__meta">
                      <HoverScrollText
                        className="playlist-card__title"
                        speed={42}
                        text={playlist.name}
                      />
                      <span>{playlistTracks.length} tracks</span>
                    </div>
                  </button>
                );
              })}
            </div>

            {selectedPlaylist ? (
              <div className="pane-stack">
                <div className="playlist-detail__header">
                  <div>
                    <h2>{selectedPlaylist.name}</h2>
                  </div>
                  <div className="inline-actions">
                    <span className="pane-chip">{playlistTracks.length} tracks</span>
                    <button
                      className="ghost-button"
                      onClick={() => void onAddPlaylistToQueue(selectedPlaylist.id)}
                      type="button"
                    >
                      Add to queue
                    </button>
                    <button
                      className="ghost-button"
                      onClick={() => void onShufflePlayPlaylist(selectedPlaylist.id)}
                      type="button"
                    >
                      Shuffle play
                    </button>
                    <button onClick={() => void onPlayPlaylist(selectedPlaylist.id)} type="button">
                      Play
                    </button>
                  </div>
                </div>

                <div className="track-table-shell track-table-shell--clamped">
                  <table className="track-table track-table--playlist">
                    <colgroup>
                      {visibleColumns.map((key) => (
                        <col
                          key={key}
                          style={{
                            width: `${buildColumnWeight(key, columnWidths, visibleColumns)}%`,
                          }}
                        />
                      ))}
                    </colgroup>
                    <thead>
                      <tr>
                        {visibleColumns.map((key, index) => {
                          const partnerKey = visibleColumns[index + 1] ?? null;

                          return (
                          <th className="track-table__header" key={key}>
                            <div className="track-table__header-inner">
                              <span title={columnLookup.get(key)?.label ?? key}>
                                {columnLookup.get(key)?.label ?? key}
                              </span>
                              {partnerKey ? (
                                <div
                                  className="track-table__resize-handle"
                                  onPointerDown={(event) =>
                                    handleResizeStart(event, key, partnerKey)
                                  }
                                  role="presentation"
                                />
                              ) : null}
                            </div>
                          </th>
                          );
                        })}
                      </tr>
                    </thead>
                    <tbody>
                      {playlistRows.length === 0 ? (
                        <tr>
                          <td className="empty-state" colSpan={Math.max(visibleColumns.length, 1)}>
                            This playlist does not have any available tracks in the current library.
                          </td>
                        </tr>
                      ) : (
                        playlistRows.map(({ track, order }) => (
                          <tr
                            className={[
                              'track-table__row--playable',
                              selectedTrackSet.has(track.id)
                                ? 'track-table__row--selected'
                                : '',
                            ]
                              .filter(Boolean)
                              .join(' ')}
                            key={track.id}
                            onClick={(event) => handleTrackClick(event, track.id)}
                            onContextMenu={(event) => handleTrackContextMenu(event, track)}
                            onDoubleClick={() => void onPlayTracks([track])}
                          >
                            {visibleColumns.map((key) => (
                              <td
                                className={[
                                  'track-table__cell',
                                  key === 'playlist_order'
                                    ? 'track-table__cell--playlist-order'
                                    : '',
                                  key === 'duration'
                                    ? 'track-table__cell--duration'
                                    : '',
                                ]
                                  .filter(Boolean)
                                  .join(' ')}
                                key={`${track.id}-${key}`}
                              >
                                {key === 'playlist_order' || key === 'duration' ? (
                                  <span className="track-table__cell-text track-table__cell-text--static">
                                    {getPlaylistColumnValue(track, key, order)}
                                  </span>
                                ) : (
                                  <HoverScrollText
                                    className="track-table__cell-text"
                                    speed={36}
                                    text={getPlaylistColumnValue(track, key, order) || '-'}
                                  />
                                )}
                              </td>
                            ))}
                          </tr>
                        ))
                      )}
                    </tbody>
                  </table>
                </div>
              </div>
            ) : null}

            {contextMenu ? (
              <div
                className="album-context-menu"
                ref={menuRef}
                style={{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }}
              >
                {contextMenu.playlistId !== 'favorites' && (
                  <>
                    <button onClick={openRenameDialog} type="button">
                      Rename
                    </button>
                    <button onClick={() => void handleDeletePlaylist()} type="button">
                      Delete
                    </button>
                  </>
                )}
                <button onClick={() => void runPlaylistAction(onRegeneratePlaylistIcon)} type="button">
                  Re-generate icon
                </button>
                <button onClick={() => void runPlaylistAction(onExportPlaylist)} type="button">
                  Export to M3U
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
                  onClick={() => void runTrackContextAction(onRemoveTracks)}
                  type="button"
                >
                  Remove
                </button>
              </div>
            ) : null}
          </>
        )}
      </SectionCard>

      {renameTarget ? (
        <div className="dialog-backdrop" onClick={() => setRenamePlaylistId(null)} role="presentation">
          <div
            aria-labelledby="playlist-rename-title"
            aria-modal="true"
            className="dialog-card playlist-rename-dialog"
            onClick={(event) => event.stopPropagation()}
            role="dialog"
          >
            <div className="dialog-card__header">
              <div>
                <p className="section-card__eyebrow">Playlist</p>
                <h3 id="playlist-rename-title">Rename Playlist</h3>
              </div>
              <button
                className="ghost-button"
                onClick={() => setRenamePlaylistId(null)}
                type="button"
              >
                Close
              </button>
            </div>
            <label className="field-label">
              Playlist name
              <input
                autoFocus
                onChange={(event) => setRenameValue(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter' && renameValue.trim()) {
                    event.preventDefault();
                    void submitRenamePlaylist();
                  }
                }}
                value={renameValue}
              />
            </label>
            <div className="inline-actions">
              <button
                className="ghost-button"
                onClick={() => setRenamePlaylistId(null)}
                type="button"
              >
                Cancel
              </button>
              <button
                disabled={!renameValue.trim()}
                onClick={() => void submitRenamePlaylist()}
                type="button"
              >
                Save
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function buildPlaylistCollage(playlist: Playlist, tracks: ScannedTrack[]) {
  const albumArts: string[] = [];
  const seenAlbums = new Set<string>();

  for (const track of tracks) {
    const artPath = track.albumArtPath;
    if (!artPath) {
      continue;
    }

    const albumId = albumIdForTrack(track);
    if (!seenAlbums.has(albumId)) {
      seenAlbums.add(albumId);
      albumArts.push(artPath);
    }
  }

  const collage: string[] = [];
  if (albumArts.length > 0) {
    const offset = playlist.collageSeed % albumArts.length;
    for (let index = 0; index < 4; index += 1) {
      collage.push(albumArts[(offset + index) % albumArts.length]);
    }
  }

  while (collage.length < 4) {
    collage.push('');
  }

  return collage;
}

function selectTrackIdsInOrder(
  orderedTracks: ScannedTrack[],
  selectedIds: string[],
): string[] {
  const selectedTrackSet = new Set(selectedIds);
  return orderedTracks
    .filter((track) => selectedTrackSet.has(track.id))
    .map((track) => track.id);
}

function buildColumnWeight(
  key: PlaylistColumnKey,
  columnWidths: Record<string, number>,
  visibleColumns: PlaylistColumnKey[],
): number {
  const total = visibleColumns.reduce(
    (sum, columnKey) => sum + (columnWidths[columnKey] ?? getDefaultColumnWidth(columnKey)),
    0,
  );

  if (total <= 0) {
    return 100 / Math.max(visibleColumns.length, 1);
  }

  return ((columnWidths[key] ?? getDefaultColumnWidth(key)) / total) * 100;
}

function normalizeColumnWidths(
  settings: TrackTableSettings,
  columns: PlaylistColumn[],
): Record<string, number> {
  const nextWidths: Record<string, number> = {};

  for (const column of columns) {
    nextWidths[column.key] = clampColumnWidth(
      column.key,
      settings.columnWidths[column.key] ?? getDefaultColumnWidth(column.key),
    );
  }

  return nextWidths;
}

function columnWidthsEqual(
  left: Record<string, number>,
  right: Record<string, number>,
): boolean {
  const leftKeys = Object.keys(left).sort();
  const rightKeys = Object.keys(right).sort();

  if (leftKeys.length !== rightKeys.length) {
    return false;
  }

  for (let index = 0; index < leftKeys.length; index += 1) {
    const key = leftKeys[index];
    if (key !== rightKeys[index]) {
      return false;
    }
    if (left[key] !== right[key]) {
      return false;
    }
  }

  return true;
}

function clampColumnWidth(key: PlaylistColumnKey, width: number): number {
  const { min, max } = getColumnWidthBounds(key);
  return Math.max(min, Math.min(max, Math.round(width)));
}

function getColumnWidthBounds(
  key: PlaylistColumnKey,
): { min: number; max: number } {
  const min =
    key === 'playlist_order'
      ? 72
      : key === 'duration'
        ? 88
        : key === 'album'
          ? 180
          : key === 'title'
            ? 220
            : key === 'composer'
              ? 180
              : 200;

  const max =
    key === 'playlist_order'
      ? 160
      : key === 'duration'
        ? 180
        : key === 'title'
          ? 1500
          : key === 'album'
            ? 1300
            : key === 'composer'
              ? 760
              : key === 'artists'
                ? 1000
                : 720;

  return {
    min,
    max,
  };
}

function resizeAdjacentColumns(
  key: PlaylistColumnKey,
  partnerKey: PlaylistColumnKey,
  startWidth: number,
  startPartnerWidth: number,
  delta: number,
): Record<string, number> {
  const primaryBounds = getColumnWidthBounds(key);
  const partnerBounds = getColumnWidthBounds(partnerKey);
  const minimumDelta = Math.max(
    primaryBounds.min - startWidth,
    startPartnerWidth - partnerBounds.max,
  );
  const maximumDelta = Math.min(
    primaryBounds.max - startWidth,
    startPartnerWidth - partnerBounds.min,
  );
  const boundedDelta =
    maximumDelta < minimumDelta
      ? 0
      : Math.max(minimumDelta, Math.min(maximumDelta, delta));

  return {
    [key]: clampColumnWidth(key, startWidth + boundedDelta),
    [partnerKey]: clampColumnWidth(
      partnerKey,
      startPartnerWidth - boundedDelta,
    ),
  };
}

function getDefaultColumnWidth(key: PlaylistColumnKey): number {
  switch (key) {
    case 'playlist_order':
      return 84;
    case 'duration':
      return 110;
    case 'album':
      return 240;
    case 'title':
      return 280;
    case 'composer':
      return 220;
    case 'artists':
      return 240;
    default:
      return 180;
  }
}

function getPlaylistColumnValue(
  track: ScannedTrack,
  key: PlaylistColumnKey,
  order: number,
): string {
  switch (key) {
    case 'playlist_order':
      return String(order);
    case 'album':
      return albumTitleForTrack(track);
    case 'title':
      return firstField(track, 'title') || track.fileName;
    case 'composer':
      return joinField(track, 'composer');
    case 'artists':
      return buildPlaylistArtists(track);
    case 'duration':
      return formatDuration(track.audio.durationMs);
    default:
      return '';
  }
}

function buildPlaylistArtists(track: ScannedTrack): string {
  const parts = [
    firstField(track, 'conductor'),
    firstField(track, 'ensemble'),
    firstField(track, 'performer') || firstField(track, 'soloist'),
  ].filter(Boolean);

  return parts.join(' / ');
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
