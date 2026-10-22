import { useMemo, useState } from 'react';
import type { Playlist, RemoteTarget, ScannedTrack } from '../../types/aria';
import { uploadPlaylistToTarget } from '../../lib/aria';
import { albumTitleForTrack, tracksForPlaylist } from '../library/view-models';

type RemotePlaylistUploadDialogProps = {
  playlist: Playlist;
  tracks: ScannedTrack[];
  targets: RemoteTarget[];
  onClose: () => void;
  onUploadStarted: (targetName: string, playlistName: string) => void;
};

export function RemotePlaylistUploadDialog({
  playlist,
  tracks,
  targets,
  onClose,
  onUploadStarted,
}: RemotePlaylistUploadDialogProps) {
  const enabledTargets = targets.filter((t) => t.isEnabled);
  const [selectedTargetId, setSelectedTargetId] = useState<string>(
    enabledTargets[0]?.id ?? '',
  );
  const [uploadFullAlbums, setUploadFullAlbums] = useState(false);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const playlistTracks = useMemo(
    () => tracksForPlaylist(tracks, playlist),
    [tracks, playlist],
  );

  const uniqueAlbums = useMemo(() => {
    const albumTitles = new Set<string>();
    for (const track of playlistTracks) {
      albumTitles.add(albumTitleForTrack(track));
    }
    return Array.from(albumTitles);
  }, [playlistTracks]);

  async function handleUpload() {
    if (!selectedTargetId) {
      setError('Please select a remote storage target.');
      return;
    }

    const target = enabledTargets.find((t) => t.id === selectedTargetId);
    if (!target) {
      setError('Selected target not found.');
      return;
    }

    if (playlistTracks.length === 0) {
      setError('Playlist has no tracks available in the library to upload.');
      return;
    }

    setIsSubmitting(true);
    setError(null);
    try {
      void uploadPlaylistToTarget(playlist.id, selectedTargetId, uploadFullAlbums).catch((err) => {
        console.error('Playlist upload failed:', err);
      });
      onUploadStarted(target.name, playlist.name);
      onClose();
    } catch (err) {
      setError(String(err));
      setIsSubmitting(false);
    }
  }

  return (
    <div className="dialog-backdrop" onClick={onClose} role="presentation">
      <div
        aria-labelledby="remote-playlist-upload-dialog-title"
        aria-modal="true"
        className="dialog-card"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
      >
        <div className="dialog-card__header">
          <div>
            <p className="section-card__eyebrow">Cloud Upload</p>
            <h3 id="remote-playlist-upload-dialog-title">Upload Playlist to Remote</h3>
          </div>
          <button className="ghost-button" onClick={onClose} type="button">
            Close
          </button>
        </div>

        {error ? (
          <div className="error-banner" style={{ marginBottom: '1rem' }}>
            {error}
          </div>
        ) : null}

        <p className="dialog-card__copy">
          Upload playlist <strong>"{playlist.name}"</strong> ({playlistTracks.length} tracks across{' '}
          {uniqueAlbums.length} {uniqueAlbums.length === 1 ? 'album' : 'albums'}). The audio files, album manifests,
          portable M3U8 playlist, and playlist manifest will be saved to the remote storage.
        </p>

        {enabledTargets.length === 0 ? (
          <div className="device-chip" style={{ marginTop: '1rem' }}>
            <span>No enabled remote storage targets found. Configure one in Settings &gt; Remote Storage.</span>
          </div>
        ) : playlistTracks.length === 0 ? (
          <div className="device-chip" style={{ marginTop: '1rem' }}>
            <span>This playlist has no tracks in the current library to upload.</span>
          </div>
        ) : (
          <>
            <div className="field-stack" style={{ marginTop: '1rem' }}>
              <label className="field-label" htmlFor="remote-target-select">
                Destination Remote
              </label>
              <select
                id="remote-target-select"
                value={selectedTargetId}
                onChange={(e) => setSelectedTargetId(e.target.value)}
              >
                {enabledTargets.map((target) => (
                  <option key={target.id} value={target.id}>
                    {target.name} ({target.backendType})
                  </option>
                ))}
              </select>
            </div>

            <div className="field-stack" style={{ marginTop: '1rem' }}>
              <span className="field-label">Upload Scope</span>
              <label style={{ display: 'flex', alignItems: 'center', gap: '0.5rem', cursor: 'pointer' }}>
                <input
                  type="radio"
                  name="uploadPlaylistScope"
                  checked={!uploadFullAlbums}
                  onChange={() => setUploadFullAlbums(false)}
                />
                <span>Playlist tracks only ({playlistTracks.length} tracks across {uniqueAlbums.length} {uniqueAlbums.length === 1 ? 'album' : 'albums'})</span>
              </label>
              <label style={{ display: 'flex', alignItems: 'center', gap: '0.5rem', cursor: 'pointer' }}>
                <input
                  type="radio"
                  name="uploadPlaylistScope"
                  checked={uploadFullAlbums}
                  onChange={() => setUploadFullAlbums(true)}
                />
                <span>Full albums for all tracks in playlist ({uniqueAlbums.length} {uniqueAlbums.length === 1 ? 'album' : 'albums'})</span>
              </label>
            </div>
          </>
        )}

        <div style={{ marginTop: '1.5rem', display: 'flex', justifyContent: 'flex-end', gap: '0.6rem' }}>
          <button className="ghost-button" onClick={onClose} type="button">
            Cancel
          </button>
          <button
            disabled={isSubmitting || enabledTargets.length === 0 || playlistTracks.length === 0}
            onClick={() => void handleUpload()}
            type="button"
          >
            {isSubmitting ? 'Starting...' : 'Upload Playlist'}
          </button>
        </div>
      </div>
    </div>
  );
}
