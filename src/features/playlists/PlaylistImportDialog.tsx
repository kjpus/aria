import { useEffect, useState } from 'react';
import type { PlaylistImportPreview, PlaylistSnapshot } from '../../types/aria';
import { getPlaylistImportPreview, commitPlaylistImport, reportDebugMessage } from '../../lib/aria';

type PlaylistImportDialogProps = {
  filePath: string;
  isOpen: boolean;
  onClose: () => void;
  onImportSuccess: (playlists: PlaylistSnapshot) => void | Promise<void>;
};

const ENCODINGS = [
  { value: 0, label: 'System Default (ANSI)' },
  { value: 65001, label: 'UTF-8' },
  { value: 1252, label: 'Windows-1252 (Western)' },
  { value: 1250, label: 'Windows-1250 (Central Europe)' },
  { value: 932, label: 'Shift-JIS (Japanese)' },
  { value: 936, label: 'GBK (Chinese)' },
];

export function PlaylistImportDialog({
  filePath,
  isOpen,
  onClose,
  onImportSuccess,
}: PlaylistImportDialogProps) {
  const [name, setName] = useState('');
  const [codepage, setCodepage] = useState<number>(0);
  const [preview, setPreview] = useState<PlaylistImportPreview | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isOpen || !filePath) {
      return;
    }

    async function loadPreview() {
      setIsLoading(true);
      setError(null);
      try {
        const data = await getPlaylistImportPreview(filePath, codepage === 0 ? undefined : codepage);
        setPreview(data);
        // Only override name on initial load
        setName((current) => current || data.name);
      } catch (err) {
        reportDebugMessage('PlaylistImportDialog:loadPreview', err);
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setIsLoading(false);
      }
    }

    void loadPreview();
  }, [filePath, codepage, isOpen]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        onClose();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onClose]);

  if (!isOpen) {
    return null;
  }

  async function handleImport() {
    if (!filePath || !name.trim() || isSubmitting) {
      return;
    }

    setIsSubmitting(true);
    setError(null);
    try {
      const playlists = await commitPlaylistImport(filePath, name.trim(), codepage);
      await onImportSuccess(playlists);
      onClose();
    } catch (err) {
      reportDebugMessage('PlaylistImportDialog:handleImport', err);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsSubmitting(false);
    }
  }

  const totalCount = preview?.tracks.length ?? 0;
  const matchedCount = preview?.tracks.filter((t) => t.trackId).length ?? 0;
  const failedCount = totalCount - matchedCount;
  const failureRate = totalCount > 0 ? failedCount / totalCount : 0;
  const canImport = preview && totalCount > 0 && name.trim().length > 0 && matchedCount > 0;

  return (
    <div className="dialog-backdrop" onClick={onClose} role="presentation">
      <div
        aria-labelledby="playlist-import-title"
        aria-modal="true"
        className="dialog-card dialog-card--wide"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
      >
        <div className="dialog-card__header">
          <div>
            <p className="section-card__eyebrow">Playlist</p>
            <h3 id="playlist-import-title">Import Playlist</h3>
          </div>
          <button className="ghost-button" onClick={onClose} type="button">
            Close
          </button>
        </div>

        <p className="dialog-card__copy" style={{ wordBreak: 'break-all' }}>
          File: {filePath}
        </p>

        <div className="dialog-sections">
          <section className="dialog-section">
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '1rem' }}>
              <label className="field-label">
                Playlist name
                <input
                  autoFocus
                  onChange={(event) => setName(event.target.value)}
                  placeholder="Imported Playlist"
                  value={name}
                />
              </label>

              <label className="field-label">
                Character encoding
                <select
                  value={codepage}
                  onChange={(event) => setCodepage(Number(event.target.value))}
                  style={{ width: '100%', height: '42px', padding: '0 0.8rem', borderRadius: '14px', border: '1px solid rgba(255, 255, 255, 0.08)', backgroundColor: 'rgba(255, 255, 255, 0.03)' }}
                >
                  {ENCODINGS.map((enc) => (
                    <option key={enc.value} value={enc.value}>
                      {enc.label}
                    </option>
                  ))}
                </select>
              </label>
            </div>
          </section>

          <section className="dialog-section">
            <div className="dialog-section__header">
              <h4>Tracks Preview</h4>
            </div>

            {isLoading ? (
              <div className="placeholder-pane">
                <strong>Loading preview...</strong>
              </div>
            ) : preview ? (
              <>
                <div className="tag-editor__summary" style={{ marginTop: '0.2rem', marginBottom: '0.8rem' }}>
                  <span className="pane-chip">
                    {preview.tracks.length} track{preview.tracks.length === 1 ? '' : 's'} total
                  </span>
                  <span
                    className="pane-chip"
                    style={{
                      backgroundColor: matchedCount === preview.tracks.length ? 'rgba(34, 197, 94, 0.15)' : 'rgba(239, 68, 68, 0.15)',
                      color: matchedCount === preview.tracks.length ? '#4ade80' : '#f87171',
                      borderColor: matchedCount === preview.tracks.length ? 'rgba(34, 197, 94, 0.3)' : 'rgba(239, 68, 68, 0.3)',
                    }}
                  >
                    {matchedCount} of {preview.tracks.length} tracks matched ({Math.round((matchedCount / (preview.tracks.length || 1)) * 100)}%)
                  </span>
                </div>

                {preview.tracks.length === 0 ? (
                  <div className="placeholder-pane">
                    <strong>No tracks found</strong>
                    <p>The playlist file appears to be empty or unrecognized.</p>
                  </div>
                ) : (
                  <div
                    style={{
                      maxHeight: '320px',
                      overflowY: 'auto',
                      border: '1px solid var(--line)',
                      borderRadius: '16px',
                      background: 'rgba(0, 0, 0, 0.15)',
                      padding: '0.25rem 0.5rem',
                    }}
                  >
                    <table style={{ width: '100%', borderCollapse: 'collapse', textAlign: 'left' }}>
                      <thead>
                        <tr style={{ borderBottom: '1px solid var(--line)' }}>
                          <th style={{ padding: '0.6rem 0.5rem', fontSize: '0.85rem', color: 'var(--muted)', width: '100px' }}>Status</th>
                          <th style={{ padding: '0.6rem 0.5rem', fontSize: '0.85rem', color: 'var(--muted)' }}>Title</th>
                          <th style={{ padding: '0.6rem 0.5rem', fontSize: '0.85rem', color: 'var(--muted)' }}>File Path</th>
                        </tr>
                      </thead>
                      <tbody>
                        {preview.tracks.map((track, i) => (
                          <tr key={i} style={{ borderBottom: '1px solid rgba(255, 255, 255, 0.02)' }}>
                            <td style={{ padding: '0.6rem 0.5rem', verticalAlign: 'middle' }}>
                              {track.trackId ? (
                                <span
                                  className="pane-chip"
                                  style={{
                                    fontSize: '0.78rem',
                                    padding: '0.15rem 0.45rem',
                                    backgroundColor: 'rgba(34, 197, 94, 0.15)',
                                    color: '#4ade80',
                                    borderColor: 'rgba(34, 197, 94, 0.25)',
                                  }}
                                >
                                  Matched
                                </span>
                              ) : (
                                <span
                                  className="pane-chip"
                                  style={{
                                    fontSize: '0.78rem',
                                    padding: '0.15rem 0.45rem',
                                    backgroundColor: 'rgba(239, 68, 68, 0.15)',
                                    color: '#f87171',
                                    borderColor: 'rgba(239, 68, 68, 0.25)',
                                  }}
                                >
                                  Missing
                                </span>
                              )}
                            </td>
                            <td
                              style={{
                                padding: '0.6rem 0.5rem',
                                verticalAlign: 'middle',
                                color: 'var(--text)',
                                fontSize: '0.9rem',
                                whiteSpace: 'nowrap',
                                overflow: 'hidden',
                                textOverflow: 'ellipsis',
                                maxWidth: '280px',
                              }}
                              title={track.title}
                            >
                              {track.title}
                            </td>
                            <td
                              style={{
                                padding: '0.6rem 0.5rem',
                                verticalAlign: 'middle',
                                color: 'var(--muted)',
                                fontSize: '0.8rem',
                                whiteSpace: 'nowrap',
                                overflow: 'hidden',
                                textOverflow: 'ellipsis',
                                maxWidth: '350px',
                              }}
                              title={track.path}
                            >
                              {track.path}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}
              </>
            ) : (
              <div className="placeholder-pane">
                <strong>No preview available</strong>
              </div>
            )}
          </section>
        </div>

        {error ? (
          <div className="export-field-dialog__error" style={{ marginTop: '1rem' }}>
            {error}
          </div>
        ) : null}

        {matchedCount === 0 && preview && preview.tracks.length > 0 ? (
          <div className="export-field-dialog__warning" style={{ marginTop: '1rem', border: '1px solid rgba(239, 68, 68, 0.3)', backgroundColor: 'rgba(239, 68, 68, 0.1)' }}>
            <strong>No matching tracks</strong>
            <p>None of the tracks in this playlist could be matched to files in your library. Please make sure the tracks are scanned first.</p>
          </div>
        ) : null}

        {preview && failedCount > 0 && failureRate <= 0.10 ? (
          <div className="export-field-dialog__warning" style={{ marginTop: '1rem', border: '1px solid rgba(214, 177, 106, 0.3)', backgroundColor: 'rgba(214, 177, 106, 0.1)', color: '#f3dfb6' }}>
            <strong>Tracks that failed to match and cannot be imported ({failedCount} of {totalCount} track{failedCount === 1 ? '' : 's'}):</strong>
            <ul style={{ margin: '0.5rem 0 0', paddingLeft: '1.2rem', maxHeight: '120px', overflowY: 'auto', fontSize: '0.85rem', color: 'var(--text)' }}>
              {preview.tracks.filter(t => !t.trackId).map((track, idx) => (
                <li key={idx} style={{ marginBottom: '0.25rem' }}>
                  <strong>{track.title}</strong> <span style={{ color: 'var(--muted)', fontSize: '0.8rem' }}>({track.path})</span>
                </li>
              ))}
            </ul>
          </div>
        ) : null}

        <div className="export-field-dialog__actions" style={{ marginTop: '1.5rem' }}>
          <button className="ghost-button" onClick={onClose} type="button">
            Cancel
          </button>
          <button
            disabled={!canImport || isSubmitting || isLoading}
            onClick={handleImport}
            type="button"
          >
            {isSubmitting ? 'Importing...' : 'Import'}
          </button>
        </div>
      </div>
    </div>
  );
}
