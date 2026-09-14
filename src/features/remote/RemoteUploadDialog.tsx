import { useState } from 'react';
import type { RemoteTarget } from '../../types/aria';
import { uploadAlbumToTarget } from '../../lib/aria';

type RemoteUploadDialogProps = {
  albumTitle: string;
  targets: RemoteTarget[];
  onClose: () => void;
  onUploadStarted: (targetName: string, albumTitle: string) => void;
};

export function RemoteUploadDialog({
  albumTitle,
  targets,
  onClose,
  onUploadStarted,
}: RemoteUploadDialogProps) {
  const enabledTargets = targets.filter((t) => t.isEnabled);
  const [selectedTargetId, setSelectedTargetId] = useState<string>(
    enabledTargets[0]?.id ?? '',
  );
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

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

    setIsSubmitting(true);
    setError(null);
    try {
      // Start upload in background
      void uploadAlbumToTarget(albumTitle, selectedTargetId).catch((err) => {
        console.error('Upload failed:', err);
      });
      onUploadStarted(target.name, albumTitle);
      onClose();
    } catch (err) {
      setError(String(err));
      setIsSubmitting(false);
    }
  }

  return (
    <div className="dialog-backdrop" onClick={onClose} role="presentation">
      <div
        aria-labelledby="remote-upload-dialog-title"
        aria-modal="true"
        className="dialog-card"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
      >
        <div className="dialog-card__header">
          <div>
            <p className="section-card__eyebrow">Cloud Upload</p>
            <h3 id="remote-upload-dialog-title">Upload Album to Remote</h3>
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
          Upload <strong>"{albumTitle}"</strong> with pre-computed classical manifest and master index.
        </p>

        {enabledTargets.length === 0 ? (
          <div className="device-chip" style={{ marginTop: '1rem' }}>
            <span>No enabled remote storage targets found. Configure one in Settings &gt; Remote Storage.</span>
          </div>
        ) : (
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
        )}

        <div style={{ marginTop: '1.5rem', display: 'flex', justifyContent: 'flex-end', gap: '0.6rem' }}>
          <button className="ghost-button" onClick={onClose} type="button">
            Cancel
          </button>
          <button
            disabled={isSubmitting || enabledTargets.length === 0}
            onClick={() => void handleUpload()}
            type="button"
          >
            {isSubmitting ? 'Starting...' : 'Upload Album'}
          </button>
        </div>
      </div>
    </div>
  );
}
