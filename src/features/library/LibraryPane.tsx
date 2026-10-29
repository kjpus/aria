import { useEffect, useMemo, useRef, useState } from 'react';
import type { MouseEvent as ReactMouseEvent } from 'react';
import { createPortal } from 'react-dom';
import { ClearableInput } from '../../components/ClearableInput';
import { RefreshIcon } from '../../components/RefreshIcon';
import { SectionCard } from '../../components/SectionCard';
import {
  EmptyArtPlaceholder,
  GridLargeIcon,
  GridMediumIcon,
  GridSmallIcon,
  PlaySimpleIcon,
  SortIcon,
} from '../../components/Icons';
import { toLocalImageSrc } from '../../lib/runtime';
import type { ScannedTrack } from '../../types/aria';
import type { AlbumGridSize } from '../../lib/ui-preferences';
import { HoverScrollText } from '../albums/HoverScrollText';
import { buildAlbumCards } from './view-models';

type LibraryPaneProps = {
  tracks: ScannedTrack[];
  selectedAlbumId: string | null;
  onOpenAlbum: (albumId: string) => void;
  onAddToPlaylist: (albumIds: string[]) => void | Promise<void>;
  onAddToQueue: (albumIds: string[]) => void | Promise<void>;
  onReplaceQueue: (albumIds: string[]) => void | Promise<void>;
  onPlayAlbum: (albumIds: string[]) => void | Promise<void>;
  onGoToDirectory: (albumIds: string[]) => void | Promise<void>;
  onUploadAlbum?: (albumTitle: string) => void;
  onRefreshRemote?: () => void;
  isSyncingRemote?: boolean;
  remoteStorageOnly?: boolean;
  gridSize?: AlbumGridSize;
  onGridSizeChange?: (size: AlbumGridSize) => void;
};

type AlbumContextMenuState = {
  albumIds: string[];
  anchorX: number;
  anchorY: number;
  x: number;
  y: number;
  maxHeight: number;
};

type AlbumSortOption = 'composer' | 'title' | 'year_desc' | 'year_asc' | 'tracks';

export function LibraryPane({
  tracks,
  selectedAlbumId,
  onOpenAlbum,
  onAddToPlaylist,
  onAddToQueue,
  onReplaceQueue,
  onPlayAlbum,
  onGoToDirectory,
  onUploadAlbum,
  onRefreshRemote,
  isSyncingRemote,
  remoteStorageOnly = false,
  gridSize = 'medium',
  onGridSizeChange,
}: LibraryPaneProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [filter, setFilter] = useState('');
  const [sortBy, setSortBy] = useState<AlbumSortOption>('composer');
  const [contextMenu, setContextMenu] = useState<AlbumContextMenuState | null>(null);
  const [selectedAlbumIds, setSelectedAlbumIds] = useState<string[]>(
    selectedAlbumId ? [selectedAlbumId] : [],
  );
  const [selectionAnchorId, setSelectionAnchorId] = useState<string | null>(
    selectedAlbumId,
  );

  const albums = useMemo(() => buildAlbumCards(tracks), [tracks]);

  const filteredAlbums = useMemo(() => {
    const query = filter.trim().toLowerCase();
    const list = query
      ? albums.filter((album) =>
          [album.title, album.composer, album.credit, album.year]
            .join(' ')
            .toLowerCase()
            .includes(query),
        )
      : [...albums];

    switch (sortBy) {
      case 'composer':
        return list.sort((a, b) => {
          const compA = (a.composer || 'ZZZ').toLowerCase();
          const compB = (b.composer || 'ZZZ').toLowerCase();
          if (compA !== compB) return compA.localeCompare(compB);
          return a.title.localeCompare(b.title);
        });
      case 'title':
        return list.sort((a, b) => a.title.localeCompare(b.title));
      case 'year_desc':
        return list.sort((a, b) => {
          const yrA = parseInt(a.year, 10) || 0;
          const yrB = parseInt(b.year, 10) || 0;
          return yrB - yrA;
        });
      case 'year_asc':
        return list.sort((a, b) => {
          const yrA = parseInt(a.year, 10) || 9999;
          const yrB = parseInt(b.year, 10) || 9999;
          return yrA - yrB;
        });
      case 'tracks':
        return list.sort((a, b) => b.trackCount - a.trackCount);
      default:
        return list;
    }
  }, [albums, filter, sortBy]);

  const selectedAlbumSet = useMemo(
    () => new Set(selectedAlbumIds),
    [selectedAlbumIds],
  );

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

  useEffect(() => {
    if (!contextMenu || !menuRef.current) {
      return;
    }

    const menuBounds = menuRef.current.getBoundingClientRect();
    const nextPlacement = resolveContextMenuPlacement(
      contextMenu.anchorX,
      contextMenu.anchorY,
      menuBounds.width,
      menuBounds.height,
    );

    if (
      nextPlacement.x !== contextMenu.x ||
      nextPlacement.y !== contextMenu.y ||
      nextPlacement.maxHeight !== contextMenu.maxHeight
    ) {
      setContextMenu((current) =>
        current
          ? {
              ...current,
              ...nextPlacement,
            }
          : current,
      );
    }
  }, [contextMenu]);

  useEffect(() => {
    const availableIds = new Set(filteredAlbums.map((album) => album.id));

    setSelectedAlbumIds((current) => {
      const next = current.filter((albumId) => availableIds.has(albumId));
      return stringListsEqual(current, next) ? current : next;
    });

    setSelectionAnchorId((current) =>
      current && availableIds.has(current) ? current : null,
    );
  }, [filteredAlbums]);

  useEffect(() => {
    if (!selectedAlbumId) {
      return;
    }

    setSelectedAlbumIds((current) =>
      current.includes(selectedAlbumId) ? current : [selectedAlbumId],
    );
    setSelectionAnchorId((current) => current ?? selectedAlbumId);
  }, [selectedAlbumId]);

  function handleAlbumClick(
    event: ReactMouseEvent<HTMLButtonElement>,
    albumId: string,
  ) {
    const orderedIds = filteredAlbums.map((album) => album.id);

    if (event.shiftKey && selectionAnchorId) {
      const anchorIndex = orderedIds.indexOf(selectionAnchorId);
      const targetIndex = orderedIds.indexOf(albumId);

      if (anchorIndex !== -1 && targetIndex !== -1) {
        const [start, end] =
          anchorIndex < targetIndex
            ? [anchorIndex, targetIndex]
            : [targetIndex, anchorIndex];
        setSelectedAlbumIds(orderedIds.slice(start, end + 1));
        return;
      }
    }

    if (event.ctrlKey || event.metaKey) {
      setSelectedAlbumIds((current) =>
        current.includes(albumId)
          ? current.filter((id) => id !== albumId)
          : [...current, albumId],
      );
      setSelectionAnchorId(albumId);
      return;
    }

    setSelectedAlbumIds([albumId]);
    setSelectionAnchorId(albumId);
    onOpenAlbum(albumId);
  }

  function openContextMenu(
    event: ReactMouseEvent<HTMLButtonElement>,
    albumId: string,
  ) {
    event.preventDefault();
    const albumIds = selectedAlbumSet.has(albumId) ? selectedAlbumIds : [albumId];
    if (!selectedAlbumSet.has(albumId)) {
      setSelectedAlbumIds([albumId]);
      setSelectionAnchorId(albumId);
    }
    setContextMenu({
      albumIds,
      ...resolveContextMenuPlacement(event.clientX, event.clientY, 240, 260),
      anchorX: event.clientX,
      anchorY: event.clientY,
    });
  }

  async function runContextAction(
    action: (albumIds: string[]) => void | Promise<void>,
  ) {
    if (!contextMenu) {
      return;
    }

    const { albumIds } = contextMenu;
    setContextMenu(null);
    await action(albumIds);
  }

  return (
    <div className="pane-stack">
      <SectionCard hideHeader>
        <div className="library-toolbar">
          <div className="library-toolbar__search">
            <ClearableInput
              className="library-toolbar__input"
              id="album-filter"
              onChange={(e) => setFilter(e.target.value)}
              onClear={() => setFilter('')}
              placeholder="Filter by composer, album, performer, or year..."
              value={filter}
            />
          </div>

          <div className="library-toolbar__controls">
            <div className="library-toolbar__sort">
              <SortIcon className="library-toolbar__sort-icon" size={14} />
              <select
                aria-label="Sort albums"
                className="library-toolbar__select"
                onChange={(e) => setSortBy(e.target.value as AlbumSortOption)}
                value={sortBy}
              >
                <option value="composer">Composer (A–Z)</option>
                <option value="title">Album Title (A–Z)</option>
                <option value="year_desc">Year (Newest first)</option>
                <option value="year_asc">Year (Oldest first)</option>
                <option value="tracks">Track Count</option>
              </select>
            </div>

            {onGridSizeChange ? (
              <div aria-label="Album cover size" className="segmented-control segmented-control--compact">
                <button
                  aria-label="Small covers"
                  aria-pressed={gridSize === 'small'}
                  className={gridSize === 'small' ? 'segmented-control__active' : ''}
                  onClick={() => onGridSizeChange('small')}
                  title="Compact covers"
                  type="button"
                >
                  <GridSmallIcon size={14} />
                </button>
                <button
                  aria-label="Medium covers"
                  aria-pressed={gridSize === 'medium'}
                  className={gridSize === 'medium' ? 'segmented-control__active' : ''}
                  onClick={() => onGridSizeChange('medium')}
                  title="Medium covers"
                  type="button"
                >
                  <GridMediumIcon size={14} />
                </button>
                <button
                  aria-label="Large covers"
                  aria-pressed={gridSize === 'large'}
                  className={gridSize === 'large' ? 'segmented-control__active' : ''}
                  onClick={() => onGridSizeChange('large')}
                  title="Large covers"
                  type="button"
                >
                  <GridLargeIcon size={14} />
                </button>
              </div>
            ) : null}

            {onRefreshRemote ? (
              <button
                className="ghost-button library-toolbar__button"
                disabled={isSyncingRemote}
                onClick={onRefreshRemote}
                title={
                  remoteStorageOnly
                    ? 'Refresh and download library metadata and tracks from remote storage'
                    : 'Sync and upload local library to remote storage'
                }
                type="button"
              >
                <RefreshIcon className={isSyncingRemote ? 'aria-spinning' : ''} />
                <span>
                  {isSyncingRemote
                    ? (remoteStorageOnly ? 'Refreshing...' : 'Syncing...')
                    : (remoteStorageOnly ? 'Refresh' : 'Sync to remote')}
                </span>
              </button>
            ) : null}

            <span className="pane-chip">{filteredAlbums.length} albums</span>
          </div>
        </div>

        <div className={`album-grid album-grid--${gridSize}`}>
          {filteredAlbums.length === 0 ? (
            <div className="empty-state-card">
              <p className="empty-state">
                {albums.length === 0
                  ? remoteStorageOnly
                    ? 'No remote music found. Click Refresh or configure a remote target in Settings.'
                    : 'No music scanned yet. Go to Settings to add your music folders.'
                  : 'No albums match the current filter.'}
              </p>
            </div>
          ) : (
            filteredAlbums.map((album) => {
              const art = toLocalImageSrc(album.artPath);
              const selected = selectedAlbumSet.has(album.id);

              return (
                <button
                  className={`album-card album-card--button${selected ? ' album-card--selected' : ''}`}
                  key={album.id}
                  onClick={(event) => handleAlbumClick(event, album.id)}
                  onContextMenu={(event) => openContextMenu(event, album.id)}
                  type="button"
                >
                  <div className="album-card__cover-wrapper">
                    {art ? (
                      <img alt="" className="album-card__art" src={art} />
                    ) : (
                      <EmptyArtPlaceholder className="album-card__art" />
                    )}
                    <button
                      aria-label={`Play ${album.title}`}
                      className="album-card__quick-play"
                      onClick={(e) => {
                        e.stopPropagation();
                        void onPlayAlbum([album.id]);
                      }}
                      title={`Play ${album.title}`}
                      type="button"
                    >
                      <PlaySimpleIcon size={14} />
                    </button>
                  </div>

                  <div className="album-card__meta">
                    {album.composer ? (
                      <HoverScrollText
                        className="album-card__composer"
                        text={album.composer}
                      />
                    ) : null}
                    <HoverScrollText
                      className="album-card__title"
                      speed={52}
                      text={album.title}
                    />
                    {album.credit ? (
                      <HoverScrollText
                        className="album-card__detail"
                        text={album.credit}
                      />
                    ) : null}
                  </div>
                  <div className="album-card__footer">
                    <span>{album.trackCount} {album.trackCount === 1 ? 'track' : 'tracks'}</span>
                    <span>{album.year || '—'}</span>
                  </div>
                </button>
              );
            })
          )}
        </div>

        {contextMenu
          ? createPortal(
              <div
                className="album-context-menu"
                ref={menuRef}
                style={{
                  left: `${contextMenu.x}px`,
                  top: `${contextMenu.y}px`,
                  maxHeight: `${contextMenu.maxHeight}px`,
                }}
              >
                <button
                  onClick={() => void runContextAction(onPlayAlbum)}
                  type="button"
                >
                  Play album
                </button>
                <button onClick={() => void runContextAction(onAddToQueue)} type="button">
                  Add to queue
                </button>
                <button
                  onClick={() => void runContextAction(onReplaceQueue)}
                  type="button"
                >
                  Replace queue
                </button>
                <button
                  onClick={() => void runContextAction(onAddToPlaylist)}
                  type="button"
                >
                  Add to playlist...
                </button>
                <button
                  onClick={() => void runContextAction(onGoToDirectory)}
                  type="button"
                >
                  Go to directory
                </button>
                {onUploadAlbum && contextMenu.albumIds.length === 1 ? (
                  <button
                    onClick={() => {
                      const album = albums.find((a) => a.id === contextMenu.albumIds[0]);
                      setContextMenu(null);
                      if (album) {
                        onUploadAlbum(album.title);
                      }
                    }}
                    type="button"
                  >
                    Upload to remote...
                  </button>
                ) : null}
              </div>,
              document.body,
            )
          : null}
      </SectionCard>
    </div>
  );
}

function resolveContextMenuPlacement(
  anchorX: number,
  anchorY: number,
  menuWidth: number,
  menuHeight: number,
) {
  const viewportWidth = window.innerWidth;
  const viewportHeight = window.innerHeight;
  const playerBar = document.querySelector('.player-bar');
  const playerTop =
    playerBar instanceof HTMLElement
      ? playerBar.getBoundingClientRect().top
      : viewportHeight;
  const padding = 12;
  const rightLimit = Math.max(padding, viewportWidth - menuWidth - padding);
  const bottomLimit = Math.max(padding, playerTop - padding);
  const left = Math.max(padding, Math.min(anchorX, rightLimit));
  const top = Math.max(
    padding,
    Math.min(anchorY, Math.max(padding, bottomLimit - menuHeight)),
  );
  const maxHeight = Math.max(120, bottomLimit - top);

  return { x: left, y: top, maxHeight };
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
