import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
} from 'react';
import { SectionCard } from '../../components/SectionCard';
import { toLocalImageSrc } from '../../lib/runtime';
import type {
  LibraryFieldMapping,
  Playlist,
  ScannedTrack,
  TrackTableSettings,
} from '../../types/aria';
import {
  albumIdForTrack,
  albumTitleForTrack,
  buildTrackColumns,
  getTrackColumnValue,
  tracksForPlaylist,
} from '../library/view-models';
import { HoverScrollText } from '../albums/HoverScrollText';

type PlaylistPaneProps = {
  playlists: Playlist[];
  tracks: ScannedTrack[];
  mappings: LibraryFieldMapping[];
  settings: TrackTableSettings;
  selectedPlaylistId: string | null;
  onSelectPlaylist: (playlistId: string) => void;
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

export function PlaylistPane({
  playlists,
  tracks,
  mappings,
  settings,
  selectedPlaylistId,
  onSelectPlaylist,
  onRenamePlaylist,
  onDeletePlaylist,
  onRegeneratePlaylistIcon,
  onExportPlaylist,
  onAddPlaylistToQueue,
  onPlayPlaylist,
  onShufflePlayPlaylist,
  onPlayTracks,
  onRemoveTracks,
  onOpenAlbum,
}: PlaylistPaneProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [contextMenu, setContextMenu] = useState<PlaylistContextMenuState | null>(null);
  const [trackContextMenu, setTrackContextMenu] =
    useState<PlaylistTrackContextMenuState | null>(null);
  const [renamePlaylistId, setRenamePlaylistId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const [selectedTrackIds, setSelectedTrackIds] = useState<string[]>([]);
  const [selectionAnchorId, setSelectionAnchorId] = useState<string | null>(null);
  const columns = useMemo(
    () =>
      buildTrackColumns(mappings).filter(
        (column) => column.key !== 'album' && column.key !== 'path',
      ),
    [mappings],
  );
  const visibleColumns = useMemo(() => {
    const preferred = settings.visibleColumns.filter(
      (key) => key !== 'album' && key !== 'path',
    );
    const filtered = preferred.filter((key) =>
      columns.some((column) => column.key === key),
    );
    return filtered.length > 0
      ? filtered
      : ['track_number', 'title', 'composer', 'conductor', 'format', 'duration'].filter((key) =>
          columns.some((column) => column.key === key),
        );
  }, [columns, settings.visibleColumns]);
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
  const groupedRows = useMemo(
    () => buildPlaylistRows(playlistTracks),
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
                  <table className="track-table track-table--grouped">
                    <thead>
                      <tr>
                        {visibleColumns.map((key) => (
                          <th className="track-table__header" key={key}>
                            {columnLookup.get(key)?.label ?? key}
                          </th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {groupedRows.length === 0 ? (
                        <tr>
                          <td className="empty-state" colSpan={Math.max(visibleColumns.length, 1)}>
                            This playlist does not have any available tracks in the current library.
                          </td>
                        </tr>
                      ) : (
                        groupedRows.map((row) =>
                          row.kind === 'album' ? (
                            <tr
                              className="track-table__album-row"
                              key={`album-${row.albumId}-${row.index}`}
                              onClick={() => onOpenAlbum(row.albumId)}
                            >
                              <td colSpan={Math.max(visibleColumns.length, 1)}>
                                <div className="track-table__album-meta">
                                  <HoverScrollText
                                    className="track-table__album-title"
                                    speed={44}
                                    text={row.title}
                                  />
                                </div>
                              </td>
                            </tr>
                          ) : (
                            <tr
                              className={[
                                'track-table__row--playable',
                                selectedTrackSet.has(row.track.id)
                                  ? 'track-table__row--selected'
                                  : '',
                              ]
                                .filter(Boolean)
                                .join(' ')}
                              key={row.track.id}
                              onClick={(event) => handleTrackClick(event, row.track.id)}
                              onContextMenu={(event) => handleTrackContextMenu(event, row.track)}
                              onDoubleClick={() => void onPlayTracks([row.track])}
                            >
                              {visibleColumns.map((key) => (
                                <td
                                  className={key === 'path' ? 'track-path' : undefined}
                                  key={`${row.track.id}-${key}`}
                                >
                                  <HoverScrollText
                                    className="track-table__cell-text"
                                    speed={36}
                                    text={getTrackColumnValue(row.track, key) || '-'}
                                  />
                                </td>
                              ))}
                            </tr>
                          ),
                        )
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
                <button onClick={openRenameDialog} type="button">
                  Rename
                </button>
                <button onClick={() => void handleDeletePlaylist()} type="button">
                  Delete
                </button>
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

type PlaylistRow =
  | { kind: 'album'; albumId: string; title: string; index: number }
  | { kind: 'track'; track: ScannedTrack };

function buildPlaylistRows(tracks: ScannedTrack[]): PlaylistRow[] {
  const rows: PlaylistRow[] = [];
  let previousAlbumId: string | null = null;

  tracks.forEach((track, index) => {
    const albumId = albumIdForTrack(track);
    if (albumId !== previousAlbumId) {
      rows.push({
        kind: 'album',
        albumId,
        title: albumTitleForTrack(track),
        index,
      });
      previousAlbumId = albumId;
    }

    rows.push({ kind: 'track', track });
  });

  return rows;
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
