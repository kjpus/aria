import { useEffect, useMemo, useRef, useState } from 'react';
import type { MouseEvent as ReactMouseEvent } from 'react';
import { createPortal } from 'react-dom';
import { ClearableInput } from '../../components/ClearableInput';
import { SectionCard } from '../../components/SectionCard';
import { toLocalImageSrc } from '../../lib/runtime';
import type { ScannedTrack } from '../../types/aria';
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
};

type AlbumContextMenuState = {
  albumIds: string[];
  anchorX: number;
  anchorY: number;
  x: number;
  y: number;
  maxHeight: number;
};

export function LibraryPane({
  tracks,
  selectedAlbumId,
  onOpenAlbum,
  onAddToPlaylist,
  onAddToQueue,
  onReplaceQueue,
  onPlayAlbum,
  onGoToDirectory,
}: LibraryPaneProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [filter, setFilter] = useState('');
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
    if (!query) {
      return albums;
    }

    return albums.filter((album) =>
      [album.title, album.composer, album.credit, album.year]
        .join(' ')
        .toLowerCase()
        .includes(query),
    );
  }, [albums, filter]);
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
          <label className="library-toolbar__label" htmlFor="album-filter">
            Filter albums
          </label>
          <ClearableInput
            className="library-toolbar__input"
            id="album-filter"
            onClear={() => setFilter('')}
            placeholder="Search album, composer, conductor or performer, year"
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
          />
          <span className="pane-chip">{filteredAlbums.length} albums</span>
        </div>

        <div className="album-grid">
          {filteredAlbums.length === 0 ? (
            <p className="empty-state">No albums match the current filter.</p>
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
                  {art ? (
                    <img alt="" className="album-card__art" src={art} />
                  ) : (
                    <div className="album-card__art album-card__art--empty">Aria</div>
                  )}
                  <div className="album-card__meta">
                    <HoverScrollText
                      className="album-card__title"
                      speed={52}
                      text={album.title}
                    />
                    {album.composer ? (
                      <HoverScrollText
                        className="album-card__detail"
                        text={album.composer}
                      />
                    ) : null}
                    {album.credit ? (
                      <HoverScrollText
                        className="album-card__detail"
                        text={album.credit}
                      />
                    ) : null}
                  </div>
                  <div className="album-card__footer">
                    <span>{album.trackCount} tracks</span>
                    <span>{album.year || 'Year unknown'}</span>
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
                  onClick={() => void runContextAction(onAddToPlaylist)}
                  type="button"
                >
                  Add to playlist
                </button>
                <button onClick={() => void runContextAction(onAddToQueue)} type="button">
                  Add to queue
                </button>
                <button
                  onClick={() => void runContextAction(onReplaceQueue)}
                  type="button"
                >
                  Put in queue
                </button>
                <button onClick={() => void runContextAction(onPlayAlbum)} type="button">
                  Play album
                </button>
                <button
                  onClick={() => void runContextAction(onGoToDirectory)}
                  type="button"
                >
                  Go to directory
                </button>
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
