import { useEffect, useState } from 'react';
import type { Playlist } from '../../types/aria';

type PlaylistPickerDialogProps = {
  suggestedName: string;
  trackCount: number;
  playlists: Playlist[];
  onAddToExisting: (playlistId: string) => void | Promise<void>;
  onClose: () => void;
  onCreate: (name: string) => void | Promise<void>;
};

export function PlaylistPickerDialog({
  suggestedName,
  trackCount,
  playlists,
  onAddToExisting,
  onClose,
  onCreate,
}: PlaylistPickerDialogProps) {
  const [name, setName] = useState(suggestedName);

  useEffect(() => {
    setName(suggestedName);
  }, [suggestedName]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        onClose();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onClose]);

  return (
    <div className="dialog-backdrop" onClick={onClose} role="presentation">
      <div
        aria-labelledby="playlist-picker-title"
        aria-modal="true"
        className="dialog-card"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
      >
        <div className="dialog-card__header">
          <div>
            <p className="section-card__eyebrow">Playlist</p>
            <h3 id="playlist-picker-title">Add To Playlist</h3>
          </div>
          <button className="ghost-button" onClick={onClose} type="button">
            Close
          </button>
        </div>

        <p className="dialog-card__copy">
          Add {trackCount} track{trackCount === 1 ? '' : 's'} to an existing playlist or create
          a new one.
        </p>

        <div className="dialog-sections">
          <section className="dialog-section">
            <div className="dialog-section__header">
              <h4>Existing playlists</h4>
            </div>
            {playlists.length === 0 ? (
              <div className="placeholder-pane">
                <strong>No playlists yet</strong>
                <p>Create the first one below.</p>
              </div>
            ) : (
              <div className="playlist-picker__list">
                {playlists.map((playlist) => (
                  <button
                    className="playlist-picker__item ghost-button"
                    key={playlist.id}
                    onClick={() => void onAddToExisting(playlist.id)}
                    type="button"
                  >
                    <strong>{playlist.name}</strong>
                    <span>{playlist.trackIds.length} tracks</span>
                  </button>
                ))}
              </div>
            )}
          </section>

          <section className="dialog-section">
            <div className="dialog-section__header">
              <h4>New playlist</h4>
            </div>
            <label className="field-label">
              Playlist name
              <input
                autoFocus
                onChange={(event) => setName(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter' && name.trim()) {
                    event.preventDefault();
                    void onCreate(name);
                  }
                }}
                placeholder="New playlist"
                value={name}
              />
            </label>
            <div className="inline-actions">
              <button disabled={!name.trim()} onClick={() => void onCreate(name)} type="button">
                Create playlist
              </button>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
