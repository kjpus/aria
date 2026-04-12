import { useEffect, useMemo, useRef, useState } from 'react';
import type { MouseEvent as ReactMouseEvent } from 'react';
import { SectionCard } from '../../components/SectionCard';
import { toLocalImageSrc } from '../../lib/runtime';
import type { ScannedTrack } from '../../types/aria';
import { HoverScrollText } from '../albums/HoverScrollText';
import { buildAlbumCards } from './view-models';

type LibraryPaneProps = {
  tracks: ScannedTrack[];
  selectedAlbumId: string | null;
  onOpenAlbum: (albumId: string) => void;
  onAddToPlaylist: (albumId: string) => void | Promise<void>;
  onAddToQueue: (albumId: string) => void | Promise<void>;
  onReplaceQueue: (albumId: string) => void | Promise<void>;
  onPlayAlbum: (albumId: string) => void | Promise<void>;
  onGoToDirectory: (albumId: string) => void | Promise<void>;
};

type AlbumContextMenuState = {
  albumId: string;
  x: number;
  y: number;
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

  function openContextMenu(
    event: ReactMouseEvent<HTMLButtonElement>,
    albumId: string,
  ) {
    event.preventDefault();
    setContextMenu({
      albumId,
      x: Math.min(event.clientX, window.innerWidth - 240),
      y: Math.min(event.clientY, window.innerHeight - 220),
    });
  }

  async function runContextAction(
    action: (albumId: string) => void | Promise<void>,
  ) {
    if (!contextMenu) {
      return;
    }

    const { albumId } = contextMenu;
    setContextMenu(null);
    await action(albumId);
  }

  return (
    <div className="pane-stack">
      <SectionCard hideHeader>
        <div className="library-toolbar">
          <label className="library-toolbar__label" htmlFor="album-filter">
            Filter albums
          </label>
          <input
            className="library-toolbar__input"
            id="album-filter"
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
              const selected = selectedAlbumId === album.id;

              return (
                <button
                  className={`album-card album-card--button${selected ? ' album-card--selected' : ''}`}
                  key={album.id}
                  onClick={() => onOpenAlbum(album.id)}
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

        {contextMenu ? (
          <div
            className="album-context-menu"
            ref={menuRef}
            style={{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }}
          >
            <button onClick={() => void runContextAction(onAddToPlaylist)} type="button">
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
      </SectionCard>
    </div>
  );
}
